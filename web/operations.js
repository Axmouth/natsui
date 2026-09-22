'use strict';
(() => {
  let preview = null,
    working = false,
    enabled = false;
  const form = $('operation-form'),
    review = $('operation-review'),
    status = $('operation-status');
  const send = (path, body) =>
    api(path, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Natsui-Request': '1',
      },
      body: JSON.stringify(body),
    });
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
    if (p.action === 'publish')
      Object.assign(p, {
        mode: $('operation-mode').value,
        subject: $('operation-subject').value.trim(),
        payload: $('operation-payload').value,
      });
    try {
      preview = await send('/api/operations/preview', p);
      $('operation-warning').textContent = preview.warning;
      $('operation-details').textContent = JSON.stringify(
        preview.proposal,
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
      const result = await send('/api/operations/apply', {
        token: preview.token,
        confirmation: $('operation-confirm').value,
      });
      reset();
      status.textContent =
        result.mode === 'jetstream'
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
    $('operation-streams').replaceChildren(
      ...(data?.snapshot.streams || [])
        .filter((s) => !/^KV_|^OBJ_/.test(s.config.name))
        .map((s) => {
          const o = element('option');
          o.value = s.config.name;
          return o;
        }),
    );
    fields();
  }
  window.addEventListener('hashchange', enter);
  enter();
  fields();
})();
