'use strict';
(() => {
  const storageKey = (suffix) =>
    `natsui-${suffix}-${staticDemo ? 'simulation' : 'live'}-${activeProfile}`;
  const read = (key, fallback) => {
    try {
      return JSON.parse(localStorage.getItem(key)) ?? fallback;
    } catch {
      return fallback;
    }
  };
  const write = (key, value) =>
    localStorage.setItem(key, JSON.stringify(value));

  const viewsPanel = element('section', null, 'panel inspector');
  const viewsForm = element('form', null, 'record-controls');
  const viewName = element('input');
  viewName.placeholder = 'Investigation name';
  viewName.maxLength = 64;
  viewName.required = true;
  viewName.setAttribute('aria-label', 'Saved investigation name');
  const saveView = element('button', 'Save current view', 'primary');
  saveView.type = 'submit';
  const viewList = element('div', null, 'view-links');
  const viewStatus = element('p');
  viewStatus.setAttribute('role', 'status');
  viewsForm.append(viewName, saveView);
  viewsPanel.append(
    element('h2', 'Saved investigations'),
    element(
      'p',
      'Saved in this browser for this profile. Stores the time window and resource selection, without observations or payloads.',
    ),
    viewsForm,
    viewList,
    viewStatus,
  );
  $('page-investigate').prepend(viewsPanel);
  function storedViews() {
    const entries = read(storageKey('views'), []);
    return Array.isArray(entries)
      ? entries
          .filter(
            (v) =>
              typeof v?.name === 'string' &&
              v.name.length <= 64 &&
              Number.isSafeInteger(v.from) &&
              Number.isSafeInteger(v.to) &&
              v.from >= 0 &&
              v.to > v.from &&
              v.to - v.from <= 21600,
          )
          .slice(0, 12)
      : [];
  }
  function renderViews() {
    viewList.replaceChildren();
    for (const view of storedViews()) {
      const group = element('span', null, 'view-links');
      const open = element('button', view.name, 'quiet');
      open.type = 'button';
      open.onclick = async () => {
        const loaded = await loadWindow(view.from, view.to);
        if (!loaded || currentPage() !== 'investigate') return;
        for (const id of [
          'investigation-stream',
          'investigation-consumer',
          'investigation-node',
        ]) {
          const value = view.selection?.[id];
          if (
            typeof value === 'string' &&
            [...$(id).options].some((o) => o.value === value)
          ) {
            $(id).value = value;
            renderInvestigation();
          }
        }
        viewStatus.textContent = `Loaded ${view.name}. Retention may have removed older observations.`;
      };
      const remove = element('button', 'Remove', 'quiet');
      remove.type = 'button';
      remove.setAttribute(
        'aria-label',
        `Remove saved investigation ${view.name}`,
      );
      remove.onclick = () => {
        try {
          write(
            storageKey('views'),
            storedViews().filter((v) => v.name !== view.name),
          );
          renderViews();
        } catch {
          viewStatus.textContent = 'Browser storage is unavailable.';
        }
      };
      group.append(open, remove);
      viewList.append(group);
    }
  }
  viewsForm.onsubmit = (event) => {
    event.preventDefault();
    const range = investigation.range,
      name = viewName.value.trim();
    if (
      !name ||
      !range ||
      range.to <= range.from ||
      range.to - range.from > 21600 ||
      investigation.loading
    ) {
      viewStatus.textContent =
        'Load a time window of up to six hours before saving.';
      return;
    }
    const views = storedViews().filter((v) => v.name !== name);
    if (views.length >= 12) {
      viewStatus.textContent =
        'Remove a saved investigation before adding another. Limit: 12.';
      return;
    }
    const selection = Object.fromEntries(
      [
        'investigation-stream',
        'investigation-consumer',
        'investigation-node',
      ].map((id) => [id, $(id).value]),
    );
    try {
      write(storageKey('views'), [...views, { name, ...range, selection }]);
      renderViews();
      viewStatus.textContent = `Saved ${name}.`;
    } catch {
      viewStatus.textContent = 'Browser storage is unavailable.';
    }
  };
  renderViews();

  const notifications = element('section', null, 'panel inspector');
  const notifyButton = element(
    'button',
    'Enable desktop notifications',
    'quiet',
  );
  notifyButton.type = 'button';
  const notifyStatus = element('p');
  notifyStatus.setAttribute('role', 'status');
  notifications.append(
    element('h2', 'Attention notifications'),
    element(
      'p',
      'Opt in to new backlog and acknowledgment-pressure transitions while this dashboard is open. At most one notification per minute for this browser profile. Notifications do not contain message payloads.',
    ),
    notifyButton,
    notifyStatus,
  );
  $('page-settings').append(notifications);
  function notifyEnabled() {
    return read(storageKey('notifications'), false) === true;
  }
  function updateNotify() {
    notifyButton.disabled =
      staticDemo || !('Notification' in globalThis) || !navigator.locks;
    notifyButton.textContent = notifyEnabled()
      ? 'Disable desktop notifications'
      : 'Enable desktop notifications';
    notifyStatus.textContent = staticDemo
      ? 'Notifications are disabled in the simulated demo.'
      : !('Notification' in globalThis)
        ? 'This browser does not support desktop notifications.'
        : !navigator.locks
          ? 'This browser cannot coordinate notifications between tabs.'
          : `Browser permission: ${Notification.permission}. ${notifyEnabled() ? 'Enabled for this profile.' : 'Disabled.'}`;
  }
  notifyButton.onclick = async () => {
    try {
      if (notifyEnabled()) write(storageKey('notifications'), false);
      else if ((await Notification.requestPermission()) === 'granted')
        write(storageKey('notifications'), true);
      updateNotify();
    } catch {
      notifyStatus.textContent =
        'Notification permission or browser storage is unavailable.';
    }
  };
  updateNotify();
  let baseline = false;
  const openedAt = Math.floor(Date.now() / 1000);
  const previousAccept = acceptIncidents;
  acceptIncidents = (result) => {
    previousAccept(result);
    if (result.status !== 'fulfilled' || !navigator.locks) return;
    const events = [...workspaceState.events];
    const first = !baseline;
    baseline = true;
    const fingerprint = (e) =>
      JSON.stringify([e.at, e.id_local, e.kind, e.stream, e.name]);
    navigator.locks
      .request(storageKey('notification-delivery'), () => {
        const key = storageKey('notification-seen');
        const stored = read(key, null);
        if (first && stored) return;
        const previous = stored || { keys: [], sent: 0 };
        const seen = new Set(Array.isArray(previous.keys) ? previous.keys : []);
        const fresh = events.filter(
          (e) =>
            e.at >= openedAt &&
            !seen.has(fingerprint(e)) &&
            ['backlog-high', 'ack-pressure'].includes(e.kind),
        );
        const active =
          !first &&
          !staticDemo &&
          notifyEnabled() &&
          'Notification' in globalThis &&
          Notification.permission === 'granted';
        const send =
          active && fresh.length && Date.now() - (previous.sent || 0) > 60000;
        try {
          write(key, {
            keys: [
              ...new Set([
                ...seen,
                ...events
                  .filter((event) => first || event.at >= openedAt)
                  .map(fingerprint),
              ]),
            ].slice(-500),
            sent: send ? Date.now() : previous.sent || 0,
          });
          if (send) {
            const event = fresh[0];
            const notification = new Notification(
              'Natsui: attention condition',
              {
                body:
                  (data?.snapshot.scope || activeProfile) +
                  ': ' +
                  fresh.length +
                  ' new backlog or acknowledgment-pressure transition(s).',
                tag: storageKey('attention'),
              },
            );
            notification.onclick = () => {
              window.focus();
              location.hash = eventUrl(event);
              notification.close();
            };
          }
        } catch {
          notifyStatus.textContent =
            'Notification delivery or browser storage is unavailable.';
        }
      })
      .catch(() => {
        notifyStatus.textContent = 'Notification coordination is unavailable.';
      });
  };

  const topology = element('section', null, 'panel inspector');
  const reloadRoutes = element('button', 'Refresh routes', 'quiet');
  reloadRoutes.type = 'button';
  const routeStatus = element('p');
  routeStatus.setAttribute('role', 'status');
  const routeGraph = element('div', null, 'route-graph');
  const routeTable = element('div', null, 'table-wrap');
  topology.append(
    element('h2', 'Cluster routes'),
    element(
      'p',
      'Reported server-to-server connections. Route pools can create several connections between a pair. Counters are per reported direction and may include multiple accounts. Unobserved peers and failed endpoints remain unknown.',
    ),
    reloadRoutes,
    routeStatus,
    routeGraph,
    routeTable,
  );
  $('page-replicas').append(topology);
  let routeLoaded = 0,
    routeLoading = false;
  async function loadRoutes() {
    if (routeLoading) return;
    routeLoading = true;
    reloadRoutes.disabled = true;
    try {
      const result = await api('/api/monitoring/routes');
      const peers = new Map(),
        links = new Map(),
        rows = [];
      for (const node of result.nodes) {
        if (node.server_id)
          peers.set(node.server_id, {
            id: node.server_id,
            name: node.name || node.server_id,
            observed: node.status === 'complete',
          });
        for (const row of node.rows || []) {
          if (!row.remote_id) continue;
          if (!peers.has(row.remote_id))
            peers.set(row.remote_id, {
              id: row.remote_id,
              name: row.remote_name || row.remote_id,
              observed: false,
            });
          const key = JSON.stringify([node.server_id, row.remote_id].sort());
          links.set(key, [node.server_id, row.remote_id]);
          rows.push([
            cell(node.name || node.server_id),
            cell(row.remote_name || row.remote_id),
            cell(number(row.rid), true),
            cell(row.rtt || '--'),
            cell(bytes(row.pending_size), true),
            cell(number(row.in_msgs), true),
            cell(number(row.out_msgs), true),
          ]);
        }
      }
      routeTable.replaceChildren(
        table(
          [
            ['Source'],
            ['Peer'],
            ['Route', true],
            ['RTT'],
            ['Pending output', true],
            ['Received', true],
            ['Sent', true],
          ],
          rows,
        ),
      );
      const complete = result.nodes.filter(
        (n) => n.status === 'complete',
      ).length;
      routeStatus.textContent = `${complete}/${result.nodes.length} endpoints complete. ${rows.length} reported routes, up to 100 per endpoint. Observed ${new Date(result.at * 1000).toLocaleTimeString()}.`;
      const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
      svg.setAttribute('viewBox', '0 0 700 320');
      svg.setAttribute('role', 'img');
      svg.setAttribute('aria-label', 'Reported cluster route topology');
      const nodes = [...peers.values()].slice(0, 64),
        positions = new Map(
          nodes.map((n, i) => [
            n.id,
            {
              x: 350 + 240 * Math.cos((i * Math.PI * 2) / nodes.length),
              y: 160 + 105 * Math.sin((i * Math.PI * 2) / nodes.length),
            },
          ]),
        );
      const draw = (tag, attrs, text) => {
        const el = document.createElementNS(svg.namespaceURI, tag);
        for (const [key, value] of Object.entries(attrs))
          el.setAttribute(key, value);
        if (text) el.textContent = text;
        svg.append(el);
        return el;
      };
      for (const [a, b] of links.values()) {
        const first = positions.get(a),
          last = positions.get(b);
        if (first && last)
          draw('line', {
            x1: first.x,
            y1: first.y,
            x2: last.x,
            y2: last.y,
            class: 'route-edge',
          });
      }
      for (const node of nodes) {
        const p = positions.get(node.id);
        draw('circle', {
          cx: p.x,
          cy: p.y,
          r: 12,
          class: node.observed ? 'route-node' : 'route-node unknown',
        });
        const text = draw(
          'text',
          {
            x: p.x,
            y: p.y + 29,
            'text-anchor': 'middle',
          },
          node.name.slice(0, 30),
        );
        const title = document.createElementNS(svg.namespaceURI, 'title');
        title.textContent = `${node.name}: ${node.observed ? 'monitoring complete' : 'monitoring unknown or incomplete'}`;
        text.append(title);
      }
      routeGraph.replaceChildren(svg);
      if (peers.size > 64)
        routeStatus.textContent += ' Diagram limited to 64 nodes.';
      routeLoaded = Date.now();
    } catch (error) {
      routeGraph.replaceChildren();
      routeTable.replaceChildren();
      routeStatus.textContent = `Routes unavailable. ${error.message}`;
    } finally {
      routeLoading = false;
      reloadRoutes.disabled = false;
    }
  }
  reloadRoutes.onclick = loadRoutes;
  const oldRender = renderWorkspace;
  renderWorkspace = () => {
    oldRender();
    if (currentPage() === 'replicas' && Date.now() - routeLoaded > 15000)
      loadRoutes();
  };

  // Fetch streaming preserves the explicit profile header used by normal reads.
  // Polling remains active whenever a proxy or browser cannot sustain the stream.
  async function updates() {
    if (staticDemo) return;
    while (true) {
      const controller = new AbortController();
      let watchdog;
      const deadline = () => {
        clearTimeout(watchdog);
        watchdog = setTimeout(() => controller.abort(), 45000);
      };
      try {
        deadline();
        const response = await fetch('/api/events', {
          headers: {
            'X-Natsui-Profile': activeProfile,
          },
          signal: controller.signal,
          cache: 'no-store',
        });
        if (response.status === 403) return;
        if (response.status === 401) {
          location.replace('/login');
          return;
        }
        if (
          !response.ok ||
          !response.body ||
          !response.headers.get('content-type')?.startsWith('text/event-stream')
        )
          throw new Error('Live stream unavailable');
        globalThis.natsuiStreamLive = true;
        const reader = response.body.getReader(),
          decoder = new TextDecoder();
        let buffer = '';
        while (true) {
          const { done, value } = await reader.read();
          if (done) break;
          deadline();
          buffer += decoder.decode(value, {
            stream: true,
          });
          if (buffer.length > 65536)
            throw new Error('Live stream frame limit exceeded');
          let end;
          while ((end = buffer.indexOf('\n\n')) !== -1) {
            const frame = buffer.slice(0, end);
            buffer = buffer.slice(end + 2);
            if (/^event: ?changed$/m.test(frame)) refresh();
          }
        }
      } catch {
        /* Refresh polling supplies the fallback without masking collection status. */
      } finally {
        controller.abort();
        clearTimeout(watchdog);
        globalThis.natsuiStreamLive = false;
        clearTimeout(timer);
        timer = setTimeout(refresh, 3000);
      }
      await new Promise((resolve) => setTimeout(resolve, 10000));
    }
  }
  updates();
})();
