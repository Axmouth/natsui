'use strict';
for (const button of document.querySelectorAll('[data-copy]')) {
  const originalLabel = button.textContent;
  let reset;
  button.addEventListener('click', async () => {
    const example = document.getElementById(button.dataset.copy);
    const status = document.getElementById('copy-status');
    clearTimeout(reset);
    button.disabled = true;
    try {
      await navigator.clipboard.writeText(example.textContent);
      button.textContent = 'Copied';
      status.textContent = 'Copied to clipboard.';
      reset = setTimeout(() => { button.textContent = originalLabel; }, 2000);
    } catch {
      button.textContent = originalLabel;
      const range = document.createRange();
      range.selectNodeContents(example);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      status.textContent = 'Example selected. Use the browser copy command.';
    } finally {
      button.disabled = false;
    }
  });
}
