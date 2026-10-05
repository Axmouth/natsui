'use strict';
const form = document.getElementById('login-form'),
  status = document.getElementById('login-status'),
  key = document.getElementById('access-key');
const flavor = localStorage.getItem('natsui-flavor') || '';
if (
  [
    '',
    'neuronic',
    'chlorophyll',
    'crimson',
    'eosin',
    'azure',
    'iris',
    'carotene',
  ].includes(flavor)
)
  document.documentElement.dataset.flavor = flavor;
document.documentElement.dataset.theme =
  localStorage.getItem('natsui-theme') === 'light' ? 'light' : 'dark';
form.onsubmit = async (event) => {
  event.preventDefault();
  const button = form.querySelector('button');
  button.disabled = true;
  status.textContent = 'Signing in...';
  try {
    const response = await fetch('/api/auth/login', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Natsui-Request': '1',
      },
      body: JSON.stringify({ key: key.value.trim() }),
    });
    key.value = '';
    if (response.ok) {
      location.replace('/');
      return;
    }
    status.textContent =
      response.status === 429
        ? 'Too many sign-in attempts. Wait one minute and try again.'
        : response.status === 401
          ? 'Access key not recognized.'
          : 'Sign-in is unavailable. Check the dashboard and try again.';
  } catch {
    status.textContent = 'The dashboard could not be reached.';
  } finally {
    button.disabled = false;
  }
};

const ticket = new URLSearchParams(location.hash.slice(1)).get('ticket');
if (ticket) {
  history.replaceState(null, '', '/login');
  status.textContent = 'Signing in with the one-time link...';
  form.hidden = true;
  (async () => {
    try {
      const r = await fetch('/api/auth/exchange', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-Natsui-Request': '1',
        },
        body: JSON.stringify({ token: ticket }),
      });
      if (r.ok) {
        location.replace('/');
        return;
      }
      status.textContent =
        'This link expired or has already been used. Generate another link or enter the access key.';
    } catch {
      status.textContent =
        'The dashboard could not be reached. Generate another link or enter the access key.';
    } finally {
      form.hidden = false;
    }
  })();
}

fetch('/api/auth/oidc').then(response=>response.ok?response.json():null).then(config=>{document.getElementById('oidc-login').hidden=!config?.enabled;}).catch(()=>{});

const natsForm = document.getElementById('nats-login-form');
fetch('/api/auth/nats')
  .then(response => response.ok ? response.json() : null)
  .then(config => {
    if (!config?.enabled || ticket) return;
    for (const profile of config.profiles) {
      const option = document.createElement('option');
      option.value = profile.id;
      option.textContent = profile.name;
      document.getElementById('nats-profile').append(option);
    }
    document.getElementById('nats-method-label').hidden = !config.certificates;
    natsForm.hidden = false;
    document.getElementById('key-login').open = false;
    document.getElementById('login-intro').textContent = config.certificates ? 'Sign in with a NATS password or client certificate.' : 'Sign in with an existing NATS username and password.';
    document.getElementById('login-note').textContent = 'Live reads and operations use this NATS user. Dashboard administration requires a separate dashboard login.';
    const shared = [config.shared_history && 'collector history', config.shared_monitoring && 'cluster monitoring'].filter(Boolean);
    document.getElementById('nats-sharing').textContent = config.session_only
      ? 'Session history starts here, grows while this dashboard is open, and is discarded at sign-out. No background collector or shared monitoring.'
      : shared.length
      ? 'This deployment also shares ' + shared.join(' and ') + ' with every NATS user who can sign in to the selected profile.'
      : 'Shared history and cluster monitoring are disabled for NATS logins.';
    document.getElementById('nats-username').focus();
  }).catch(() => {});
const method = document.getElementById('nats-method');
const certificateInput = document.getElementById('nats-certificate');
const keyInput = document.getElementById('nats-private-key');
method.onchange = () => {
  const certificate = method.value === 'certificate';
  document.getElementById('nats-password-fields').hidden = certificate;
  document.getElementById('nats-certificate-fields').hidden = !certificate;
  for (const id of ['nats-username', 'nats-password']) {
    const input = document.getElementById(id);
    input.disabled = certificate;
    input.required = !certificate;
    input.value = '';
  }
  for (const input of [certificateInput, keyInput]) {
    input.disabled = !certificate;
    input.required = certificate;
    input.value = '';
  }
};
natsForm.onsubmit = async event => {
  event.preventDefault();
  const button = natsForm.querySelector('button');
  const password = document.getElementById('nats-password');
  const profile = document.getElementById('nats-profile').value;
  button.disabled = true;
  status.textContent = 'Signing in with NATS...';
  try {
    let credentials;
    if (method.value === 'certificate') {
      const certificate = certificateInput.files[0], privateKey = keyInput.files[0];
      if (!certificate || !privateKey || certificate.size > 98304 || privateKey.size > 32768) {
        status.textContent = 'Select a certificate up to 96 KiB and an unencrypted PEM key up to 32 KiB.';
        return;
      }
      const bytes = new Uint8Array(await certificate.arrayBuffer());
      let binary = '';
      for (const byte of bytes) binary += String.fromCharCode(byte);
      credentials = { certificate: btoa(binary), private_key: await privateKey.text() };
    } else {
      credentials = { username: document.getElementById('nats-username').value, password: password.value };
    }
    const response = await fetch('/api/auth/nats', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-Natsui-Request': '1' },
      body: JSON.stringify({ profile, ...credentials }),
    });
    if (response.ok) {
      sessionStorage.setItem('natsui-profile', profile);
      location.replace('/');
      return;
    }
    status.textContent = response.status === 429
      ? 'Too many sign-in attempts. Wait one minute and try again.'
      : (await response.text()).slice(0, 250) || 'NATS sign-in is unavailable.';
  } catch { status.textContent = 'The dashboard could not be reached.'; }
  finally { password.value = ''; certificateInput.value = ''; keyInput.value = ''; button.disabled = false; }
};
