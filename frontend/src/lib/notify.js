// Toast + dialog primitives — a tiny dependency-free pub/sub store that the
// app uses instead of the browser's blocking alert()/confirm()/prompt().
//
//   import { toast, confirmDialog, promptDialog } from '../lib/notify.js';
//   toast('Saved', 'success');
//   if (!(await confirmDialog({ message: 'Delete X?', danger: true }))) return;
//   const name = await promptDialog({ message: 'Name', defaultValue: 'x' });
//
// `<Toaster/>` + `<DialogHost/>` (components/Notify.jsx) render the state and
// are mounted once in App. The dialog host is an accessible modal (role=dialog,
// focus-trap, Esc-to-cancel).

// ── toasts ──────────────────────────────────────────────────────────────────
let nextId = 0;
let toasts = [];
const toastSubs = new Set();
const emit = () => toastSubs.forEach((fn) => fn(toasts));

export function subscribeToasts(fn) {
  toastSubs.add(fn);
  fn(toasts);
  return () => toastSubs.delete(fn);
}

/** Show a transient toast. `kind`: 'info' | 'success' | 'error'. ms<=0 = sticky. */
export function toast(message, kind = 'info', ms = 4500) {
  const id = ++nextId;
  toasts = [...toasts, { id, message: String(message), kind }];
  emit();
  if (ms > 0) setTimeout(() => dismissToast(id), ms);
  return id;
}
export function dismissToast(id) {
  toasts = toasts.filter((t) => t.id !== id);
  emit();
}

// ── confirm / prompt dialogs ──────────────────────────────────────────────
// One host at a time. openDialog hands the request (plus a `resolve`) to the
// subscribed host; if none is mounted it resolves to a safe default (cancel).
let dialogSub = null;
export function subscribeDialog(fn) {
  dialogSub = fn;
  return () => {
    if (dialogSub === fn) dialogSub = null;
  };
}
function openDialog(req) {
  return new Promise((resolve) => {
    if (!dialogSub) {
      resolve(req.kind === 'prompt' ? null : false);
      return;
    }
    dialogSub({ ...req, resolve });
  });
}

/** Async replacement for `confirm()`. Resolves true/false. */
export function confirmDialog({ title, message, confirmLabel, cancelLabel, danger = false }) {
  return openDialog({ kind: 'confirm', title, message, confirmLabel, cancelLabel, danger });
}
/** Async replacement for `prompt()`. Resolves the entered string, or null on cancel. */
export function promptDialog({ title, message, defaultValue = '', confirmLabel, cancelLabel }) {
  return openDialog({ kind: 'prompt', title, message, defaultValue, confirmLabel, cancelLabel });
}
