'use strict';
// Capability states come from the server, which matches them against this login's NATS
// grants. The labels and notes below explain each one. NATS still decides every request.
const NatsuiAccess = (() => {
  const catalog = {
    own_permissions: ['Read own permissions', "Reveals only this login's own grants. Lets the dashboard show what the login can use without probing."],
    list_streams: ['List all streams', 'Returns every stream in the account with configuration and state. Deny it when a login should see only the streams it is granted.'],
    node_metrics: ['Node metrics', 'Server-wide process metrics and totals, with no per-client detail. An application account also needs the system account export and an import.'],
    connections: ['Connections', "Lists client connections with names, addresses and subscriptions of other identities. Suited to operators."],
    subscriptions: ['Live subscriptions', "Lists subscription subjects from every identity, including other systems' subjects. Suited to operators."],
    routes: ['Route traffic', "Per-route counters. A request can also list route interest across accounts, which reveals other streams' subjects."],
    create_stream: ['Create streams', 'Creates storage in the account. Usually an administrator task.'],
    consumer_inventory: ['Consumer progress', 'Consumer state and backlog for this stream. Read-only.'],
    read_messages: ['Read messages', "Shows retained payloads and headers. Grant when this login may read the stream's data."],
    publish: ['Publish', "Publishes to the stream's subjects. Applications using this login may already have it."],
    republish: ['Republish a record', "Replays a retained record as a new message. Needs message read and publish on the stream's subjects."],
    purge: ['Purge', 'Removes retained records. Consumers lose unprocessed work in the purged range. Not reversible.'],
    delete_message: ['Delete a record', 'Removes single records. Consumers that have not received a record skip it.'],
    create_consumer: ['Create consumers', 'Creates durable pull consumers on this stream.'],
    delete_stream: ['Delete stream', 'Removes the stream with its records and consumers. Usually an administrator task.'],
    pause_consumer: ['Pause and resume', 'Stops and restarts delivery for a period. Applications keep running.'],
    delete_consumer: ['Delete consumer', "Removes the consumer's position and delivery state."],
  };
  const actions = {
    create_stream: 'create_stream', create_consumer: 'create_consumer', delete_stream: 'delete_stream',
    delete_consumer: 'delete_consumer', pause_consumer: 'pause_consumer', resume_consumer: 'pause_consumer',
    purge_stream: 'purge', delete_message: 'delete_message', republish_message: 'republish', publish: 'publish',
  };
  const capabilities = () => data?.snapshot?.capabilities;
  const writes = () => !!data?.dashboard?.writes_enabled;
  function list(stream, consumer) {
    const c = capabilities();
    if (!c) return null;
    const { account, name } = splitStream(stream);
    const same = (x) => x.stream === name && (x.account || '') === account;
    if (consumer) return c.consumers.find((x) => same(x) && x.consumer === consumer)?.capabilities || null;
    if (stream) return c.streams.find(same)?.capabilities || null;
    return c.cluster;
  }
  // true or false when known, null when this login's grants are unknown.
  function allowed(id, stream, consumer) {
    const entry = list(stream, consumer)?.find((x) => x.id === id);
    if (!entry) return null;
    if (entry.write && !writes()) return false;
    return entry.granted;
  }
  function action(name, stream, consumer) {
    const id = actions[name];
    if (!id) return null;
    if (name === 'create_stream') return allowed(id);
    if (!stream) {
      const c = capabilities();
      if (!c) return writes() ? null : false;
      const pools = id.endsWith('consumer') && id !== 'create_consumer' ? c.consumers : c.streams;
      const states = pools.map((p) => allowed(id, p.account ? `${p.account}/${p.stream}` : p.stream, p.consumer));
      return states.includes(true) ? true : states.length && states.every((s) => s === false) ? false : writes() ? null : false;
    }
    return allowed(id, stream, id.endsWith('consumer') && id !== 'create_consumer' ? consumer : undefined);
  }
  function button(label, params, state) {
    if (state === false) return null;
    const b = element('button', label, 'quiet');
    b.type = 'button';
    b.onclick = () => (location.hash = '#operations?' + new URLSearchParams(params));
    return b;
  }
  function renderActions() {
    if (!data?.snapshot) return;
    const q = filters();
    const page = currentPage();
    const host = (id) => $(id);
    if (page === 'stream' && host('stream-actions')) {
      const stream = q.get('name');
      host('stream-actions').replaceChildren(...[
        ['Publish', 'publish'], ['Purge', 'purge_stream'], ['Create consumer', 'create_consumer'], ['Delete stream', 'delete_stream'],
      ].map(([label, name]) => button(label, { action: name, stream }, action(name, stream))).filter(Boolean));
    }
    if (page === 'consumer' && host('consumer-actions')) {
      const stream = q.get('stream'), consumer = q.get('name');
      host('consumer-actions').replaceChildren(...[
        ['Pause', 'pause_consumer'], ['Resume', 'resume_consumer'], ['Delete consumer', 'delete_consumer'],
      ].map(([label, name]) => button(label, { action: name, stream, consumer }, action(name, stream, consumer))).filter(Boolean));
    }
    if (page === 'messages' && host('message-actions')) {
      const stream = $('message-stream').value, sequence = $('message-seq').value;
      const shown = !!sequence && !!$('message-result').querySelector('h3');
      host('message-actions').replaceChildren(...(shown ? [
        ['Republish', 'republish_message'], ['Delete record', 'delete_message'],
      ].map(([label, name]) => button(label, { action: name, stream, sequence }, action(name, stream))).filter(Boolean) : []));
    }
  }
  function status(entry) {
    if (entry.granted === null) return 'Unknown';
    if (!entry.granted) return 'Not granted';
    if (entry.id === 'node_metrics' && entry.reachable === false) return 'Granted, no server answered';
    if (entry.write && !writes()) return 'Granted, writes off in natsui';
    return 'Available';
  }
  function requirement(entry) {
    const host = element('span');
    entry.needs.forEach((subject, i) => {
      if (i) host.append(', ');
      host.append(element('code', subject));
    });
    if (entry.id === 'node_metrics' && entry.reachable === false) host.append(element('span', 'System account export and an import into this account', 'secondary'));
    if (entry.write && !writes()) host.append(element('span', 'NATSUI_ALLOW_WRITES=1 on the dashboard', 'secondary'));
    if (!entry.needs.length) host.append(element('span', 'The stream captures no subjects', 'secondary'));
    return host;
  }
  const holder = (child) => {
    const td = element('td');
    td.append(child);
    return td;
  };
  function rows(entries) {
    return entries.map((entry) => {
      const [label, note] = catalog[entry.id] || [entry.id, ''];
      const state = status(entry);
      const badge = element('span', state, `badge ${state === 'Available' ? 'good' : state === 'Unknown' ? '' : 'warn'}`);
      return [cell(label), holder(badge), holder(requirement(entry)), cell(note)];
    });
  }
  const heads = [['Capability'], ['Status'], ['Publish grants'], ['Consider']];
  function capabilityTable(entries) {
    const wrap = element('div', null, 'table-wrap');
    wrap.append(table(heads, rows(entries)));
    return wrap;
  }
  function render() {
    const host = $('access-capabilities');
    if (!host || currentPage() !== 'coverage') return;
    const c = capabilities();
    if (!c) {
      host.hidden = true;
      return;
    }
    host.hidden = false;
    const open = new Set([...host.querySelectorAll('details[open]')].map((d) => d.dataset.key));
    const intro = c.known
      ? `Matched against this login's NATS permissions. ${c.restricted ? 'Grants not listed here are not needed by the dashboard.' : 'This login has no permission restrictions.'}`
      : "This login's permissions are unknown, so each capability is shown with what it needs. Grant $SYS.REQ.USER.INFO to evaluate them.";
    const parts = [element('h2', 'What this login can do'), element('p', intro)];
    if (c.known)
      parts.push(
        element(
          'p',
          "How this is checked: at sign-in the dashboard requests $SYS.REQ.USER.INFO, and NATS replies with this login's publish allow and deny lists. Each capability's subjects are matched against those lists with NATS wildcard rules. A denial always wins, and a wildcard requirement needs one grant that covers it. No request is sent to test a grant, except one node metrics request at sign-in to confirm a server answers. The lists are read once per sign-in, so sign in again after changing permissions. NATS still decides every request.",
        ),
      );
    if (!writes()) parts.push(element('p', 'Write capabilities also need NATSUI_ALLOW_WRITES=1 on this dashboard.'));
    parts.push(element('h3', 'Cluster'), capabilityTable(c.cluster));
    for (const s of c.streams) {
      const d = element('details');
      d.dataset.key = 'stream:' + (s.account || '') + '/' + s.stream;
      d.open = open.has(d.dataset.key) || c.streams.length === 1;
      d.append(element('summary', `Stream ${s.account ? s.account + '/' : ''}${s.stream}`), capabilityTable(s.capabilities));
      parts.push(d);
    }
    if (c.consumers.length) {
      const d = element('details');
      d.dataset.key = 'consumers';
      d.open = open.has('consumers');
      d.append(element('summary', `Consumers (${c.consumers.length})`));
      for (const x of c.consumers) d.append(element('h3', `${x.account ? x.account + '/' : ''}${x.stream} / ${x.consumer}`), capabilityTable(x.capabilities));
      parts.push(d);
    }
    host.replaceChildren(...parts);
  }
  return { allowed, action, render, renderActions, catalog };
})();
globalThis.NatsuiAccess = NatsuiAccess;
