'use strict';
(() => {
  const host = $('dashboard-users'),
    message = $('users-status'),
    form = $('user-create'),
    list = $('user-list'),
    dialog = $('user-key-dialog');
  let busy = false;
  const options = {
    headers: {
      'Content-Type': 'application/json',
      'X-Natsui-Request': '1',
    },
    method: 'POST',
  };
  function reveal(value) {
    if (!value.key) return;
    $('user-key-title').textContent = 'Access key for ' + value.id;
    $('user-key-value').value = value.key;
    dialog.showModal();
  }
  $('user-key-close').onclick = () => dialog.close();
  dialog.addEventListener('close', () => {
    $('user-key-value').value = '';
  });
  async function change(row, action, role) {
    if (busy) return;
    if (
      !confirm(
        action === 'delete'
          ? 'Delete ' + row.id + ' and revoke their access?'
          : 'Apply ' +
              action +
              ' for ' +
              row.id +
              '? Existing sessions will be revoked.',
      )
    )
      return;
    busy = true;
    try {
      reveal(
        await api('/api/users/' + encodeURIComponent(row.id), {
          ...options,
          body: JSON.stringify({
            revision: row.revision,
            action,
            ...(role ? { role } : {}),
          }),
        }),
      );
      message.textContent = 'Access updated. Existing sessions were revoked.';
      await load();
    } catch (e) {
      message.textContent = e.message + ' Refresh the list before retrying.';
    } finally {
      busy = false;
    }
  }
  async function load() {
    const result = await api('/api/users');
    if (!result.users.length) {
      empty(
        'user-list',
        'No named users. The configured bootstrap key retains administrator access.',
      );
      return;
    }
    list.replaceChildren(
      table(
        [['User'], ['Role'], ['Status'], ['Actions']],
        result.users.map((row) => {
          const role = element('select');
          role.setAttribute('aria-label', 'Role for ' + row.id);
          for (const value of ['viewer', 'operator', 'admin']) {
            const option = element('option', value);
            option.value = value;
            role.append(option);
          }
          role.value = row.role;
          const roleCell = cell();
          roleCell.append(role);
          const apply = element('button', 'Save role', 'quiet');
          apply.onclick = () => change(row, 'role', role.value);
          roleCell.append(apply);
          const actions = cell();
          for (const [action, label] of [
            [
              row.enabled ? 'disable' : 'enable',
              row.enabled ? 'Disable' : 'Enable',
            ],
            ['rotate', 'Rotate key'],
            ['delete', 'Delete'],
          ]) {
            const b = element('button', label, 'quiet');
            b.onclick = () => change(row, action);
            actions.append(b);
          }
          return [
            cell(row.id),
            roleCell,
            cell(row.enabled ? 'Enabled' : 'Disabled'),
            actions,
          ];
        }),
      ),
    );
  }
  form.onsubmit = async (event) => {
    event.preventDefault();
    if (busy) return;
    busy = true;
    const button = form.querySelector('button');
    button.disabled = true;
    try {
      const result = await api('/api/users', {
        ...options,
        body: JSON.stringify({
          id: $('user-name').value,
          role: $('user-role').value,
        }),
      });
      form.reset();
      reveal(result);
      await load();
      message.textContent =
        'User created. Store the access key before closing the dialog.';
    } catch (e) {
      message.textContent = e.message;
    } finally {
      busy = false;
      button.disabled = false;
    }
  };
  $('users-reload').onclick = () =>
    load().catch((e) => (message.textContent = e.message));
  async function enter() {
    if (currentPage() !== 'settings') return;
    if (staticDemo) {
      message.textContent =
        'Dashboard identities are available in authenticated live deployments.';
      form.hidden = true;
      return;
    }
    try {
      const identity = await api('/api/auth/me');
      $('settings-form')
        .querySelectorAll('input,button')
        .forEach((e) => (e.disabled = identity.role !== 'admin'));
      if (!identity.enabled) {
        message.textContent =
          'Trusted local access. Configure a dashboard key to enable named users.';
        form.hidden = true;
        return;
      }
      message.textContent = 'Signed in with ' + identity.role + ' access.';
      const admin = identity.role === 'admin';
      form.hidden = !admin;
      $('users-reload').hidden = !admin;
      if (admin) await load();
      else list.replaceChildren();
    } catch (e) {
      message.textContent = e.message;
    }
  }
  window.addEventListener('hashchange', enter);
  enter();
})();
