'use strict';
(() => {
  let preview = null,
    working = false,
    enabled = false;
  const form = $('operation-form'),
    review = $('operation-review'),
    status = $('operation-status');
  // An account/stream name routes the operation to that account's API prefix.
  const send = (path, body, account) => {
    const headers = {
      'Content-Type': 'application/json',
      'X-Natsui-Request': '1',
    };
    if (account) headers['X-Natsui-Account'] = account;
    return api(path, { method: 'POST', headers, body: JSON.stringify(body) });
  };
  let reviewedAccount = '';
  function reset() {
    preview = null;
    review.hidden = true;
    $('operation-confirm').value = '';
  }
  function fields() {
    reset();
    const action = $('operation-action').value;
    form.querySelectorAll('[data-operation]').forEach((section) => {
      section.hidden = !section.dataset.operation.split(' ').includes(action);
      section
        .querySelectorAll('input,select,textarea')
        .forEach((input) => (input.disabled = section.hidden || !enabled));
    });
    $('operation-stream').required =
      action !== 'publish' || $('operation-mode').value === 'jetstream';
    $('operation-consumer').required = action.endsWith('consumer');
    streams(action);
  }
  // Suggestions follow this login's grants. Typed names are still sent, and NATS decides.
  function streams(action) {
    $('operation-streams').replaceChildren(
      ...(data?.snapshot.streams || [])
        .filter((s) => !/^KV_|^OBJ_/.test(s.config.name))
        .filter(
          (s) =>
            action === 'create_stream' ||
            action.endsWith('consumer') && action !== 'create_consumer' ||
            NatsuiAccess.action(action, streamId(s)) !== false,
        )
        .map((s) => {
          const o = element('option');
          o.value = streamId(s);
          return o;
        }),
    );
  }
  function gate() {
    for (const option of $('operation-action').options) {
      const state = NatsuiAccess.action(option.value);
      option.disabled = state === false;
      option.title =
        state === false
          ? 'Not available to this login. Data coverage lists what enables it.'
          : '';
    }
    const select = $('operation-action');
    if (select.selectedOptions[0]?.disabled) {
      const first = [...select.options].find((o) => !o.disabled);
      if (first) select.value = first.value;
    }
  }
  function prefill() {
    const q = filters();
    if (!q.get('action')) return;
    $('operation-action').value = q.get('action');
    for (const [key, id] of [
      ['stream', 'operation-stream'],
      ['consumer', 'operation-consumer'],
      ['sequence', 'operation-sequence'],
    ])
      if (q.get(key)) $(id).value = q.get(key);
    if (q.get('action') === 'publish') $('operation-mode').value = 'jetstream';
  }
  form.addEventListener('input', reset);
  $('operation-action').onchange = fields;
  $('operation-mode').onchange = fields;
  $('operation-cancel').onclick = reset;
  form.onsubmit = async (event) => {
    event.preventDefault();
    if (working || !enabled) return;
    working = true;
    $('operation-preview').disabled = true;
    reset();
    const p = {
      action: $('operation-action').value,
      stream: $('operation-stream').value.trim(),
    };
    if (p.action.endsWith('consumer'))
      p.consumer = $('operation-consumer').value.trim();
    if (p.action === 'create_stream')
      p.config = {
        subjects: $('operation-subjects')
          .value.split(',')
          .map((s) => s.trim())
          .filter(Boolean),
        storage: $('operation-storage').value,
        retention: $('operation-retention').value,
        num_replicas: Number($('operation-replicas').value),
        max_msgs: Number($('operation-max').value),
      };
    if (p.action === 'create_consumer')
      p.config = {
        filter_subject: $('operation-filter').value.trim(),
        deliver_policy: $('operation-deliver').value,
      };
    if (p.action === 'pause_consumer') {
      const date=new Date($('operation-pause-until').value);
      if(!Number.isFinite(date.getTime())){status.textContent='Choose a valid pause end time.';working=false;$('operation-preview').disabled=!enabled;return;}
      p.config={pause_until:date.toISOString()};
    }
    if (p.action === 'purge_stream') {
      const filter = $('operation-purge-filter').value.trim();
      p.config = filter ? { filter } : {};
    }
    if (p.action === 'delete_message' || p.action === 'republish_message')
      p.sequence = Number($('operation-sequence').value);
    if (p.action === 'republish_message' && $('operation-republish-subject').value.trim())
      p.subject = $('operation-republish-subject').value.trim();
    if (p.action === 'publish')
      Object.assign(p, {
        mode: $('operation-mode').value,
        subject: $('operation-subject').value.trim(),
        payload: $('operation-payload').value,
      });
    try {
      const target = splitStream(p.stream);
      preview = await send(
        '/api/operations/preview',
        { ...p, stream: target.name },
        target.account,
      );
      reviewedAccount = target.account;
      $('operation-warning').textContent = preview.warning;
      $('operation-details').textContent = JSON.stringify(
        preview.record
          ? { ...preview.proposal, record: preview.record }
          : preview.proposal,
        null,
        2,
      );
      $('operation-confirm-name').textContent =
        'Confirmation: ' + preview.confirmation;
      review.hidden = false;
      status.textContent = 'Review is valid for 60 seconds.';
    } catch (e) {
      status.textContent = e.message;
    } finally {
      working = false;
      $('operation-preview').disabled = !enabled;
    }
  };
  $('operation-apply').onclick = async () => {
    if (!preview || working) return;
    working = true;
    $('operation-apply').disabled = true;
    try {
      const result = await send(
        '/api/operations/apply',
        { token: preview.token, confirmation: $('operation-confirm').value },
        reviewedAccount,
      );
      reset();
      status.textContent =
        result.purged != null
          ? `Purged ${number(result.purged)} records from ${result.resource}.`
          : result.subject && result.sequence != null
            ? `Republished to ${result.subject}, stored in ${result.stream} at sequence ${number(result.sequence)}. Application processing is not tracked.`
            : result.mode === 'jetstream'
          ? 'Stored in ' +
            result.stream +
            ' at sequence ' +
            number(result.sequence) +
            '. Application processing is not tracked.'
          : result.mode === 'core'
            ? 'Core publish sent. Acceptance is unconfirmed, including permission checks. Storage and processing are not verified.'
            : result.verified
              ? 'Operation applied and verified for ' + result.resource + '.'
              : 'Request accepted. Current resource state could not be verified.';
      if (!result.audit_saved)
        status.textContent += ' The outcome could not be saved to Activity.';
      $('operation-payload').value = '';
    } catch (e) {
      reset();
      status.textContent = e.message + ' No automatic retry was sent.';
    } finally {
      working = false;
      $('operation-apply').disabled = false;
    }
  };
  async function enter() {
    if (currentPage() !== 'operations') return;
    if (staticDemo) {
      enabled = false;
      $('operation-access').textContent =
        'The standalone demo does not send NATS writes.';
    } else {
      try {
        const capability = await api('/api/editing');
        enabled = capability.enabled;
        $('operation-access').textContent = enabled
          ? 'Native writes enabled for this session. Each operation requires review.'
          : 'Writes are disabled. Operator access and NATSUI_ALLOW_WRITES=1 are required.';
      } catch (e) {
        enabled = false;
        status.textContent = e.message;
      }
    }
    form
      .querySelectorAll('input,select,textarea,button')
      .forEach((e) => (e.disabled = !enabled));
    prefill();
    gate();
    fields();
  }
  window.addEventListener('hashchange', enter);
  enter();
  fields();
})();
