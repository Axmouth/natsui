'use strict';
(() => {
  if (staticDemo) return;
  const host = document.querySelector('.topbar');
  if (!host) return;
  const select = element('select', null, 'profile-select');
  select.setAttribute('aria-label', 'Active connection profile');
  select.hidden = true;
  host.prepend(select);
  api('/api/profiles')
    .then((result) => {
      for (const profile of result.profiles) {
        const option = element('option', profile.name);
        option.value = profile.id;
        select.append(option);
      }
      select.value = result.selected;
      select.hidden =
        result.profiles.length < 2 &&
        result.profiles.some((profile) => profile.id === result.selected);
    })
    .catch(() => {});
  select.onchange = async () => {
    select.disabled = true;
    try {
      await api('/api/profiles/select', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-Natsui-Request': '1',
        },
        body: JSON.stringify({ id: select.value }),
      });
      sessionStorage.setItem('natsui-profile', select.value);
      location.hash = '#overview';
      location.reload();
    } catch (e) {
      select.disabled = false;
      select.title = e.message;
    }
  };
})();
