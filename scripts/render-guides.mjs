import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';

const pages = [
  ['SETUP.md', 'setup.html', 'Setup guide'],
  ['ANSIBLE.md', 'ansible.html', 'Ansible deployment'],
  ['SHARED_ACCESS.md', 'shared-access.html', 'Shared access'],
  ['MONITORING_SECURITY.md', 'monitoring-security.html', 'Monitoring security'],
  ['PROFILES.md', 'profiles.html', 'Connection profiles'],
  ['MANAGED.md', 'managed.html', 'Managed NATS'],
];
const escape = (text) =>
  text
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;');
function link(url) {
  if (/^(https?:|#)/.test(url)) return url;
  const [file, fragment] = url.split('#');
  const page = pages.find(([source]) => source === file);
  if (page) return page[1] + (fragment ? '#' + fragment : '');
  if (file.startsWith('../deploy/') && !file.endsWith('/ansible'))
    return (
      'try/' +
      file.slice('../deploy/'.length) +
      (fragment ? '#' + fragment : '')
    );
  if (file.endsWith('/ansible'))
    return 'https://github.com/Axmouth/natsui/tree/main/deploy/ansible';
  return (
    'https://github.com/Axmouth/natsui/blob/main/' +
    path.posix.normalize('docs/' + file) +
    (fragment ? '#' + fragment : '')
  );
}
function inline(text) {
  const tokens = [];
  const stash = (value) => {
    tokens.push(value);
    return '\u0001' + (tokens.length - 1) + '\u0001';
  };
  text = text.replace(/`([^`]+)`/g, (_, code) =>
    stash('<code>' + escape(code) + '</code>'),
  );
  text = text.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_, label, url) =>
    stash('<a href="' + escape(link(url)) + '">' + escape(label) + '</a>'),
  );
  text = escape(text).replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  return text.replace(
    /\u0001(\d+)\u0001/g,
    (_, index) => tokens[Number(index)],
  );
}
export function renderMarkdown(markdown, page) {
  const lines = markdown.split(/\r?\n/),
    output = [],
    slugs = new Map();
  let index = 0,
    command = 0;
  while (index < lines.length) {
    const line = lines[index];
    if (!line.trim()) {
      index++;
      continue;
    }
    if (/^(```|~~~)/.test(line)) {
      const fence = line.slice(0, 3),
        body = [];
      index++;
      while (index < lines.length && !lines[index].startsWith(fence))
        body.push(lines[index++]);
      index++;
      const id = page + '-command-' + ++command;
      output.push(
        '<div class="command"><div class="command-heading"><span>Example</span><button data-copy="' +
          id +
          '">Copy</button></div><pre><code id="' +
          id +
          '">' +
          escape(body.join('\n')) +
          '</code></pre></div>',
      );
      continue;
    }
    const heading = line.match(/^(#{1,6})\s+(.+)$/);
    if (heading) {
      const base = heading[2]
        .toLowerCase()
        .replace(/[^\w -]/g, '')
        .replaceAll(' ', '-');
      const count = slugs.get(base) || 0;
      slugs.set(base, count + 1);
      const id = base + (count ? '-' + count : '');
      const level = Math.min(heading[1].length + 1, 6);
      output.push(
        '<h' +
          level +
          ' id="' +
          id +
          '">' +
          inline(heading[2]) +
          '</h' +
          level +
          '>',
      );
      index++;
      continue;
    }
    if (line.startsWith('|') && /^\|[\s:|-]+\|$/.test(lines[index + 1] || '')) {
      const cells = (value) =>
        value
          .trim()
          .replace(/^\||\|$/g, '')
          .split('|')
          .map((cell) => inline(cell.trim()));
      output.push(
        '<div class="table-wrap"><table><thead><tr>' +
          cells(line)
            .map((c) => '<th>' + c + '</th>')
            .join('') +
          '</tr></thead><tbody>',
      );
      index += 2;
      while (index < lines.length && lines[index].startsWith('|'))
        output.push(
          '<tr>' +
            cells(lines[index++])
              .map((c) => '<td>' + c + '</td>')
              .join('') +
            '</tr>',
        );
      output.push('</tbody></table></div>');
      continue;
    }
    if (/^[-*] /.test(line) || /^\d+\. /.test(line)) {
      const ordered = /^\d+\. /.test(line),
        tag = ordered ? 'ol' : 'ul',
        pattern = ordered ? /^\d+\. / : /^[-*] /;
      output.push('<' + tag + '>');
      while (index < lines.length && pattern.test(lines[index]))
        output.push(
          '<li>' + inline(lines[index++].replace(pattern, '')) + '</li>',
        );
      output.push('</' + tag + '>');
      continue;
    }
    const paragraph = [line];
    index++;
    while (
      index < lines.length &&
      lines[index].trim() &&
      !/^(#|```|~~~|\||[-*] |\d+\. )/.test(lines[index])
    )
      paragraph.push(lines[index++]);
    output.push('<p>' + inline(paragraph.join(' ')) + '</p>');
  }
  return output.join('\n');
}
export async function buildGuides(root, out) {
  for (const [source, destination, title] of pages) {
    const markdown = await readFile(path.join(root, 'docs', source), 'utf8');
    const body = renderMarkdown(markdown, destination.replace('.html', ''));
    const nav = pages
      .map(([, file, label]) => '<a href="' + file + '">' + label + '</a>')
      .join('');
    await writeFile(
      path.join(out, destination),
      '<!doctype html>\n<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>' +
        title +
        ' - Natsui</title><link rel="icon" href="assets/kitten.svg"><link rel="stylesheet" href="site.css"><script src="site.js" defer></script></head><body><header><a class="wordmark" href="./">natsui<span>.</span></a><nav><a href="demo/">Browser demo</a><a href="https://github.com/Axmouth/natsui">GitHub</a></nav></header><main class="setup-guide"><section class="hero"><p class="eyebrow">DEPLOYMENT GUIDE</p><h1>' +
        title +
        '</h1></section><nav class="guide-nav" aria-label="Deployment guides">' +
        nav +
        '</nav><section>' +
        body +
        '</section></main><footer><a href="./">Natsui</a><span id="copy-status" role="status" aria-live="polite"></span></footer></body></html>\n',
    );
  }
}
