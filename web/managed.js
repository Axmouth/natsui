'use strict';
(() => {
  let current = null,
    preview = null,
    busy = false;
  const status = $('managed-status'),
    form = $('managed-form'),
    review = $('managed-review');
  const node = () => $('managed-node').value;
  const send = (action, body) =>
    api('/api/managed/' + encodeURIComponent(node()) + '/' + action, {
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
    $('managed-confirm').value = '';
  }
  async function load() {
    reset();
    if (!node()) return;
    try {
      current = await api(
        '/api/managed/' + encodeURIComponent(node()) + '/status',
      );
      $('managed-current').textContent = JSON.stringify(current, null, 2);
      const jwt = current.identity_model === 'jwt-account';
      setExamples(jwt);
      $('managed-credential-note').textContent = jwt
        ? 'Signed credentials may work immediately wherever the account is trusted. Local revocations require distribution before broker access changes.'
        : 'Credentials are generated when omitted. An optional 64-character hexadecimal password can provision the same credential on another node. Passwords are stored as bcrypt hashes by the controller.';
      status.textContent = jwt
        ? 'JWT account authority. Local changes and resolver acceptance are reported separately.'
        : current.running
          ? 'NATS process running. Configured and observed settings are shown separately.'
          : 'NATS process stopped.';
      form.hidden = false;
    } catch (e) {
      current = null;
      form.hidden = true;
      status.textContent = e.message;
    }
  }
  $('managed-node').onchange = load;
  $('managed-load').onclick = load;
  form.oninput = reset;
  form.onsubmit = async (event) => {
    event.preventDefault();
    if (!current || busy) return;
    busy = true;
    reset();
    try {
      const value = JSON.parse($('managed-proposal').value);
      value.revision = current.revision;
      preview = await send('preview', value);
      $('managed-diff').textContent = JSON.stringify(preview, null, 2);
      $('managed-confirm-label').textContent =
        'Type ' + (preview.confirmation || 'APPLY') + ' to confirm';
      review.hidden = false;
      status.textContent =
        'Operation reviewed. Check the effects and confirmation text before applying.';
    } catch (e) {
      status.textContent = e.message;
    } finally {
      busy = false;
    }
  };
  $('managed-apply').onclick = async () => {
    if (!preview || busy) return;
    busy = true;
    try {
      const result = await send('apply', {
        token: preview.token,
        confirmation: $('managed-confirm').value,
      });
      reset();
      $('managed-proposal').value = '';
      await load();
      $('managed-result').textContent = JSON.stringify(result, null, 2);
      $('managed-result-dialog').showModal();
    } catch (e) {
      reset();
      status.textContent = e.message + ' No automatic retry was sent.';
    } finally {
      busy = false;
    }
  };
  $('managed-close').onclick = () => $('managed-result-dialog').close();
  $('managed-result-dialog').addEventListener('close', () => {
    $('managed-result').textContent = '';
  });
  const examples = {
    settings: {
      action: 'settings',
      settings: { max_connections: 10000 },
    },
    create_user: {
      action: 'create_user',
      name: 'app-reader',
      permissions: { publish: [], subscribe: ['app.>'] },
    },
    rotate_user: { action: 'rotate_user', name: 'app-reader' },
    permissions: {
      action: 'permissions',
      name: 'app-reader',
      permissions: { publish: [], subscribe: ['app.>'] },
    },
    delete_user: { action: 'delete_user', name: 'app-reader' },
    restart: { action: 'restart' },
  };
  const jwtExamples = {
    create_user: {
      action: 'create_user',
      name: 'app-reader',
      expiry_hours: 24,
      permissions: { publish: [], subscribe: ['app.>'] },
    },
    revoke_user: { action: 'revoke_user', name: 'app-reader' },
    export_account: { action: 'export_account' },
    publish_account: { action: 'publish_account' },
  };
  let activeExamples = examples,
    exampleMode = null;
  function setExamples(jwt) {
    if (exampleMode === jwt) return;
    exampleMode = jwt;
    activeExamples = jwt ? jwtExamples : examples;
    $('managed-example').replaceChildren(
      ...Object.keys(activeExamples).map((key) => {
        const option = element('option', key.replaceAll('_', ' '));
        option.value = key;
        return option;
      }),
    );
    $('managed-example').onchange();
  }
  $('managed-example').onchange = () => {
    $('managed-proposal').value = JSON.stringify(
      activeExamples[$('managed-example').value],
      null,
      2,
    );
    reset();
  };
  $('managed-example').onchange();
  async function enter() {
    if (currentPage() !== 'settings' || staticDemo) return;
    try {
      const identity = await api('/api/auth/me');
      if (identity.role !== 'admin' || !identity.enabled) {
        status.textContent =
          'Authenticated dashboard administrator access is required.';
        return;
      }
      const value = await api('/api/managed');
      $('managed-node').replaceChildren(
        ...value.nodes.map((id) => {
          const o = element('option', id);
          o.value = id;
          return o;
        }),
      );
      if (!value.nodes.length) {
        status.textContent =
          'No deployment controller configured. Standard mode attaches to unmodified NATS.';
        return;
      }
      await load();
    } catch (e) {
      status.textContent = e.message;
    }
  }
  window.addEventListener('hashchange', enter);
  enter();
})();
