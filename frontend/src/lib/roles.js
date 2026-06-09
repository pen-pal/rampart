// RBAC helpers — mirror the backend role model (rampart-core::role::Role).
//
// The backend is the source of truth (it 403s mutations); these helpers
// drive UX gating so users don't see buttons they can't use.
//
// Roles: 'admin' | 'editor' | 'readonly'.
//   admin    → everything
//   editor   → all monitoring CRUD, but no admin surfaces
//   readonly → read only; no mutations anywhere

/** True if the user may perform mutating actions (admin or editor). */
export function canWrite(user) {
  const role = user?.role;
  return role === 'admin' || role === 'editor';
}

/** True only for the admin role. */
export function isAdmin(user) {
  return user?.role === 'admin';
}

/** True for the readonly role (convenience for "view only" copy). */
export function isReadonly(user) {
  return user?.role === 'readonly';
}
