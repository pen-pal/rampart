// Spanish — real translations for high-traffic keys; everything else
// falls back to the English source via the `...en` spread. See en.js for
// the rationale behind the flat dotted-key layout.
import en from './en.js';

const es = {
  ...en,
  'dashboard.title':        'Panel',
  'dashboard.add_monitor':  'Añadir monitor',
  'dashboard.status_page':  'Página de estado',
  'dashboard.sign_out':     'Cerrar sesión',
  'dashboard.all_monitors': 'Todos los monitores',
  'dashboard.loading':      'Cargando…',
  'common.cancel':          'Cancelar',
  'common.save':            'Guardar',
  'common.delete':          'Eliminar',
  'common.clear':           'Limpiar',
  'locale.picker.label':    'Idioma',
};

export default es;
