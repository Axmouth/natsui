'use strict';
(() => {
  let selected = null,
    next = null,
    generation = 0;
  const status = $('bucket-status');
  async function inspect(row) {
    const target = selected;
    if (!target) return;
    const serial = ++generation;
    status.textContent = 'Reading latest retained revision...';
    try {
      const value = await api(
        '/api/buckets/' +
          target.kind +
          '/' +
          encodeURIComponent(target.bucket) +
          '/entry?' +
          new URLSearchParams({ key: row.key }),
      );
      if (serial !== generation) return;
      $('bucket-dialog-title').textContent = row.name;
      $('bucket-value').textContent = JSON.stringify(value, null, 2);
      $('bucket-dialog').showModal();
      status.textContent =
        'Revision read from storage. No application consumer was pulled or acknowledged.';
    } catch (e) {
      if (serial === generation) status.textContent = e.message;
    }
  }
  async function keys(bucket, offset = 0) {
    selected = bucket;
    const serial = ++generation;
    status.textContent = 'Reading retained subjects...';
    try {
      const value = await api(
        '/api/buckets/' +
          bucket.kind +
          '/' +
          encodeURIComponent(bucket.bucket) +
          '?offset=' +
          offset,
      );
      if (serial !== generation) return;
      $('bucket-title').textContent =
        bucket.bucket +
        ' / ' +
        (bucket.kind === 'kv' ? 'Key Value' : 'Object Store');
      $('bucket-entries').hidden = false;
      $('bucket-keys').replaceChildren(
        table(
          [['Key or object'], ['Retained revisions', true], ['State']],
          value.rows.map((row) => {
            const name = cell(),
              button = element('button', row.name, 'name-button');
            button.onclick = () => inspect(row);
            name.append(button);
            return [
              name,
              cell(number(row.retained_revisions), true),
              cell('Inspect latest revision'),
            ];
          }),
        ),
      );
      next = value.next_offset;
      $('bucket-next').hidden = next == null;
      status.textContent =
        value.rows.length +
        ' retained subjects shown. Tombstones may be present.';
    } catch (e) {
      if (serial === generation) status.textContent = e.message;
    }
  }
  async function load() {
    const serial = ++generation;
    try {
      const result = await api('/api/buckets');
      if (serial !== generation) return;
      status.textContent =
        'Bucket inventory: ' +
        result.status +
        (staticDemo ? ' / simulated' : '');
      if (!result.buckets.length) {
        empty(
          'bucket-list',
          result.status === 'complete'
            ? 'No buckets observed.'
            : 'Bucket inventory is incomplete or unavailable.',
        );
        return;
      }
      $('bucket-list').replaceChildren(
        table(
          [['Bucket'], ['Type'], ['Stored', true], ['Replicas', true]],
          result.buckets.map((bucket) => {
            const name = cell(),
              button = element('button', bucket.bucket, 'name-button');
            button.onclick = () => keys(bucket);
            name.append(button);
            return [
              name,
              cell(bucket.kind === 'kv' ? 'Key Value' : 'Object Store'),
              cell(bytes(bucket.bytes), true),
              cell(number(bucket.replicas), true),
            ];
          }),
        ),
      );
    } catch (e) {
      status.textContent = e.message;
    }
  }
  $('bucket-next').onclick = () => {
    if (selected && next != null) keys(selected, next);
  };
  $('bucket-reload').onclick = load;
  $('bucket-close').onclick = () => $('bucket-dialog').close();
  $('bucket-dialog').addEventListener('close', () => {
    $('bucket-value').textContent = '';
  });
  function enter() {
    if (currentPage() === 'buckets') load();
    else generation++;
  }
  window.addEventListener('hashchange', enter);
  enter();
})();
