// This controller lives outside the sandboxed authored document.
const frame = document.querySelector('iframe');
const stage = document.querySelector('.stage');
const pins = document.querySelector('.pins');
const selector = document.querySelector('#screens');
const list = document.querySelector('#threads');
const dialog = document.querySelector('dialog');
const messages = document.querySelector('#messages');
const reply = document.querySelector('textarea');
const status = document.querySelector('#status');
const picker = document.querySelector('#anchors');
let threads = [];
let screen = config.screens[0];
let selected = null;
let newAnchor = null;
let commentMode = false;
let scale = 1;
let screenRoot = null;
let anchorElements = new Map();
let scheduled = false;
let opener = null;
let pending = null;
let retryCounter = 0;
const drafts = new Map();
const retries = new Map();

function send(action) {
  const body = JSON.stringify({ instance: config.instance, document: config.id, ...action });
  if (window.ipc) window.ipc.postMessage(body);
  else if (window.chrome?.webview) window.chrome.webview.postMessage(body);
}
function button(text, action) {
  const result = document.createElement('button');
  result.type = 'button'; result.textContent = text; result.onclick = action;
  return result;
}
function anchorElement(anchor) {
  return anchor ? anchorElements.get(anchor) : screenRoot;
}
function available(thread) {
  const target = config.screens.find(item => item.id === thread.screen);
  return !!target && (!thread.anchor || target.anchors.some(a => a.id === thread.anchor));
}
function saveDraft() { if (dialog.open) drafts.set(selected || `new:${screen.id}:${newAnchor || ''}`, reply.value); }
function positionDialog(source) {
  if (!source?.isConnected) return;
  const rect = source.getBoundingClientRect();
  dialog.style.margin = '0';
  dialog.style.left = `${Math.max(12, Math.min(innerWidth - dialog.offsetWidth - 12, rect.left))}px`;
  dialog.style.top = `${Math.max(12, Math.min(innerHeight - dialog.offsetHeight - 12, rect.bottom + 6))}px`;
}
function openThread(thread, source) {
  if (pending && !pending.failed) { status.textContent = 'Saving comment…'; return; }
  saveDraft(); selected = thread.id; newAnchor = null; opener = source;
  document.querySelector('#thread-title').textContent = thread.label || 'Screen comment';
  messages.replaceChildren();
  if (!available(thread)) messages.append(Object.assign(document.createElement('p'), { textContent: 'Anchor removed', className: 'muted' }));
  for (const message of thread.messages) {
    const item = document.createElement('div'); item.className = 'message';
    const author = document.createElement('div'); author.className = 'author';
    author.textContent = message.authorId === 'user' ? 'You' : message.authorLabel;
    item.append(author, document.createTextNode(message.body)); messages.append(item);
  }
  reply.value = drafts.get(selected) || '';
  const resolve = document.querySelector('#resolve');
  resolve.hidden = false; resolve.textContent = thread.resolved ? 'Reopen' : 'Resolve';
  document.querySelector('#stage').hidden = false;
  if (!dialog.open) dialog.showModal();
  positionDialog(source);
  reply.focus();
  send({ type: 'read', thread: thread.id, message: thread.messages.at(-1)?.id });
}
function openNew(anchor, source) {
  if (pending && !pending.failed) { status.textContent = 'Saving comment…'; return; }
  saveDraft(); selected = null; newAnchor = anchor; opener = source;
  const label = screen.anchors.find(a => a.id === anchor)?.label || screen.name;
  document.querySelector('#thread-title').textContent = label;
  messages.replaceChildren(); reply.value = drafts.get(`new:${screen.id}:${anchor || ''}`) || '';
  document.querySelector('#resolve').hidden = true;
  document.querySelector('#stage').hidden = true;
  if (!dialog.open) dialog.showModal();
  positionDialog(source);
  reply.focus();
}
function close() {
  saveDraft(); dialog.close();
  if (opener?.isConnected) opener.focus(); else document.querySelector('#comments').focus();
}
function draw() {
  scheduled = false;
  if (!frame.contentDocument?.body) return;
  const viewport = document.querySelector('.viewport');
  scale = Math.min(1, Math.max(1, viewport.clientWidth - 32) / screen.width);
  frame.style.width = `${screen.width}px`; frame.style.height = `${screen.height}px`;
  frame.style.transform = `scale(${scale})`;
  stage.style.width = `${screen.width * scale}px`; stage.style.height = `${screen.height * scale}px`;
  pins.replaceChildren();
  const groups = new Map();
  for (const thread of threads.filter(t => !t.resolved && t.screen === screen.id)) {
    const element = anchorElement(thread.anchor);
    if (!element || !element.getClientRects().length) continue;
    const rect = element.getBoundingClientRect();
    if (rect.bottom <= 0 || rect.right <= 0 || rect.top >= screen.height || rect.left >= screen.width) continue;
    const x = Math.max(0, Math.min(screen.width * scale - 28, rect.right * scale - 14));
    const y = Math.max(0, Math.min(screen.height * scale - 28, rect.top * scale));
    const key = `${Math.round(x / 28)}:${Math.round(y / 28)}`;
    if (!groups.has(key)) groups.set(key, { x, y, threads: [] });
    groups.get(key).threads.push(thread);
  }
  for (const group of groups.values()) {
    const dot = button('', () => {
      if (group.threads.length === 1) openThread(group.threads[0], dot);
      else { list.hidden = false; const target = list.querySelector(`[data-thread-index="${threads.indexOf(group.threads[0])}"]`); target?.focus(); }
    });
    dot.className = 'pin' + (group.threads.some(t => t.unread) ? ' unread' : '');
    dot.dataset.threadIds = group.threads.map(t => t.id).join(' ');
    dot.style.left = `${group.x}px`; dot.style.top = `${group.y}px`;
    dot.setAttribute('aria-label', group.threads.length === 1 ? `Comment: ${group.threads[0].label}${group.threads[0].unread ? ', unread' : ''}` : `${group.threads.length} comments here`);
    pins.append(dot);
  }
  if (dialog.open) {
    if (selected && opener?.classList.contains('pin')) {
      opener = Array.from(pins.children).find(pin => pin.dataset.threadIds.split(' ').includes(selected)) || document.querySelector('#comments');
    }
    positionDialog(opener);
  }
}
function schedule() { if (!scheduled) { scheduled = true; requestAnimationFrame(draw); } }
function chooseScreen() {
  screen = config.screens.find(item => item.id === selector.value) || config.screens[0];
  if (!frame.contentDocument?.body) return;
  for (const node of frame.contentDocument.querySelectorAll('[data-wireframe-screen]')) {
    node.style.setProperty('display', node.dataset.wireframeScreen === screen.id ? '' : 'none', 'important');
    if (node.dataset.wireframeScreen === screen.id) {
      node.style.width = `${screen.width}px`; node.style.height = `${screen.height}px`;
      screenRoot = node;
    }
  }
  // Authored scripts cannot mutate the DOM. Cache element references once
  // per screen selection; scroll/resize only reads the active anchors' bounds.
  anchorElements = new Map(Array.from(screenRoot.querySelectorAll('[data-wireframe-id]')).map(node => [node.dataset.wireframeId, node]));
  if (screenRoot.dataset.wireframeId) anchorElements.set(screenRoot.dataset.wireframeId, screenRoot);
  picker.replaceChildren(new Option('Screen comment', ''));
  for (const anchor of screen.anchors) picker.add(new Option(anchor.label || anchor.id, anchor.id));
  schedule();
}
function renderList() {
  list.replaceChildren();
  for (const [index, thread] of threads.entries()) {
    const text = `${thread.unread ? 'Unread · ' : ''}${thread.resolved ? 'Resolved · ' : ''}${!available(thread) ? 'Anchor removed · ' : ''}${thread.label || 'Screen comment'}`;
    const item = button(text, () => {
      selector.value = thread.screen; chooseScreen(); openThread(thread, item);
    });
    item.dataset.threadIndex = index; list.append(item);
  }
  if (!threads.length) list.textContent = 'No comments yet.';
}
window.goddardWireframe = {
  setThreads(value, request) {
    threads = value; renderList(); schedule();
    if (request && pending?.key === request) {
      drafts.delete(pending.draft); retries.delete(pending.draft); pending = null; reply.value = ''; status.textContent = 'Comment saved';
      document.querySelector('#save').disabled = false; reply.disabled = false;
      const current = threads.find(thread => thread.id === selected);
      if (current && dialog.open) openThread(current, opener); else close();
    }
  },
  setError(message) { status.textContent = message; if (pending) pending.failed = true; document.querySelector('#save').disabled = false; reply.disabled = false; },
  setTheme(variables) {
    for (const [key, value] of Object.entries(variables)) {
      document.documentElement.style.setProperty(key, value);
      frame.contentDocument?.documentElement.style.setProperty(key, value);
    }
  }
};
for (const item of config.screens) selector.add(new Option(item.name, item.id));
selector.onchange = chooseScreen;
document.querySelector('#comments').onclick = () => { list.hidden = !list.hidden; schedule(); };
document.querySelector('#comment-mode').onclick = event => {
  commentMode = !commentMode; event.currentTarget.setAttribute('aria-pressed', String(commentMode));
  picker.hidden = !commentMode; document.querySelector('#new-comment').hidden = !commentMode;
};
document.querySelector('#new-comment').onclick = event => openNew(picker.value || null, event.currentTarget);
document.querySelector('#close').onclick = close;
dialog.addEventListener('cancel', event => { event.preventDefault(); close(); });
dialog.addEventListener('click', event => { if (event.target === dialog) { const r = dialog.getBoundingClientRect(); if (event.clientX < r.left || event.clientX > r.right || event.clientY < r.top || event.clientY > r.bottom) close(); } });
document.querySelector('#save').onclick = () => {
  const body = reply.value; if (!body.trim()) { reply.focus(); return; }
  const draft = selected || `new:${screen.id}:${newAnchor || ''}`;
  pending = retries.get(draft);
  if (!pending || pending.body !== body) pending = { key: `${config.instance}:${Date.now()}:${++retryCounter}`, body, draft };
  retries.set(draft, pending);
  pending.failed = false;
  document.querySelector('#save').disabled = true; reply.disabled = true; status.textContent = 'Saving comment…';
  send({ type: selected ? 'reply' : 'add', thread: selected, screen: screen.id, anchor: newAnchor, body, key: pending.key });
};
document.querySelector('#resolve').onclick = () => {
  const thread = threads.find(t => t.id === selected);
  if (thread) send({ type: 'resolve', thread: selected, resolved: !thread.resolved });
  close();
};
document.querySelector('#stage').onclick = () => {
  if (reply.value.trim()) { status.textContent = 'Save your reply before adding this thread to a message.'; return; }
  send({ type: 'stage', thread: selected }); close();
};
frame.addEventListener('load', () => {
  const doc = frame.contentDocument;
  if (!doc) return;
  const style = doc.createElement('style'); style.textContent = config.base;
  doc.head.prepend(style);
  window.goddardWireframe.setTheme(config.theme);
  doc.addEventListener('submit', event => event.preventDefault(), true);
  doc.addEventListener('click', event => {
    // Static controls must never navigate, submit, or replace this document.
    event.preventDefault();
    if (!commentMode) return;
    const target = event.target.closest('[data-wireframe-id]');
    openNew(target?.dataset.wireframeId || null, document.querySelector('#comment-mode'));
  }, true);
  doc.addEventListener('scroll', schedule, true);
  new ResizeObserver(schedule).observe(doc.body);
  chooseScreen(); send({ type: 'ready' });
});
new ResizeObserver(schedule).observe(document.querySelector('.viewport'));
window.goddardWireframe.setTheme(config.theme);
frame.srcdoc = config.html;
