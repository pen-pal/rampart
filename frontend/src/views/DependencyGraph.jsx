import React, { useMemo, useState } from 'react';
import { Loader2, AlertCircle, Share2 } from 'lucide-react';
import { api, useApi, statusToClass } from '../lib/api.js';
import { t } from '../lib/i18n.js';
import SubViewHeader from '../components/SubViewHeader.jsx';

// ─────────────────────────────────────────────────────────────────────────────
// Dependency graph — a READ-ONLY visualisation of how monitors depend on each
// other. Built entirely client-side from data already exposed by the API:
//
//   • api.monitors.list()            → the node set (id, name, current_status)
//   • api.dependencies.list(id)       → { parents, children } per monitor
//
// We fan `dependencies.list` across every monitor to assemble the edge set.
// An edge means "the parent silences the dependent (child) when it goes down",
// so we draw the arrow FROM parent TO child — i.e. in the direction the outage
// propagates. There is NO backend change: this view reuses existing endpoints.
//
// Layout is a hand-rolled layered (Sugiyama-ish) DAG layout — roots (monitors
// nothing depends on, i.e. with no incoming edge) sit in the top layer, and
// each node drops to one layer below its deepest parent. Cycles (if any slip
// past the backend's guard) are tolerated: a visited-set keeps the longest-path
// pass terminating. No graph library — plain SVG nodes + edges.
// ─────────────────────────────────────────────────────────────────────────────

const css = `
  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --up:#10b981; --up-soft:#d1fae5;
    --down:#ef4444; --down-soft:#fee2e2;
    --warn:#f59e0b; --warn-soft:#fef3c7;
    --maint:#6366f1;
    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif;
    min-height: 100vh;
  }
  .rampart * { box-sizing: border-box; }
  .mono { font-family: 'JetBrains Mono', monospace; }
  .card { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; }
  .btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 7px 12px; border-radius: 8px; cursor: pointer;
    font-size: 13px; font-weight: 500; line-height: 1;
    background: var(--surface); border: 1px solid var(--border); color: var(--text-2);
    font-family: inherit; text-decoration: none;
  }
  .btn:hover { background: var(--surface-2); color: var(--text); border-color: var(--border-2); }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; }
  .dep-node { cursor: pointer; }
  .dep-node:hover .dep-node-box { stroke-width: 2.5; filter: brightness(0.97); }
  .dep-legend-dot { width: 10px; height: 10px; border-radius: 3px; display: inline-block; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .spin { animation: spin 1s linear infinite; }
`;

// Map a monitor status to a fill/stroke pair using the theme palette.
function statusColors(status) {
  switch (statusToClass(status)) {
    case 'up':    return { fill: 'var(--up-soft)',     stroke: 'var(--up)',     text: '#065f46' };
    case 'down':  return { fill: 'var(--down-soft)',   stroke: 'var(--down)',   text: '#991b1b' };
    case 'warn':  return { fill: 'var(--warn-soft)',   stroke: 'var(--warn)',   text: '#92400e' };
    case 'maint': return { fill: '#e0e7ff',            stroke: 'var(--maint)',  text: '#3730a3' };
    case 'paused':
    default:      return { fill: 'var(--surface-2)',   stroke: 'var(--border-2)', text: 'var(--text-2)' };
  }
}

// Layer dimensions (SVG units). Tuned for legibility, not density.
const NODE_W = 150;
const NODE_H = 46;
const COL_GAP = 60;   // horizontal gap between nodes in the same layer
const ROW_GAP = 90;   // vertical gap between layers
const PAD = 40;       // outer padding inside the SVG canvas

// Assign every node a layer (depth) via longest-path from any root.
// roots = nodes with no incoming edge. Returns Map<id, layer>.
function assignLayers(nodeIds, edges) {
  const incoming = new Map(nodeIds.map(id => [id, 0]));
  const adj = new Map(nodeIds.map(id => [id, []])); // parent -> [children]
  for (const e of edges) {
    if (!adj.has(e.from) || !incoming.has(e.to)) continue;
    adj.get(e.from).push(e.to);
    incoming.set(e.to, (incoming.get(e.to) || 0) + 1);
  }
  const layer = new Map(nodeIds.map(id => [id, 0]));
  // BFS-ish longest path. Guard against cycles with a per-node visit cap.
  const visits = new Map(nodeIds.map(id => [id, 0]));
  const queue = nodeIds.filter(id => (incoming.get(id) || 0) === 0);
  // If everything is in a cycle (no roots), seed with all nodes at layer 0.
  const seed = queue.length ? queue : [...nodeIds];
  const work = [...seed];
  while (work.length) {
    const id = work.shift();
    if ((visits.get(id) || 0) > nodeIds.length) continue; // cycle breaker
    visits.set(id, (visits.get(id) || 0) + 1);
    const ld = layer.get(id);
    for (const child of adj.get(id) || []) {
      if (layer.get(child) < ld + 1) {
        layer.set(child, ld + 1);
        work.push(child);
      }
    }
  }
  return layer;
}

// Build {nodes, edges, width, height} positioned for SVG rendering.
function buildLayout(monitors, edges) {
  const nodeIds = monitors.map(m => m.id);
  const byId = new Map(monitors.map(m => [m.id, m]));
  const layer = assignLayers(nodeIds, edges);

  // Group node ids by layer, preserving monitor list order for stable layout.
  const layers = new Map();
  for (const m of monitors) {
    const l = layer.get(m.id) || 0;
    if (!layers.has(l)) layers.set(l, []);
    layers.get(l).push(m.id);
  }
  const sortedLayerKeys = [...layers.keys()].sort((a, b) => a - b);

  const widestCount = Math.max(1, ...[...layers.values()].map(a => a.length));
  const canvasW = PAD * 2 + widestCount * NODE_W + (widestCount - 1) * COL_GAP;
  const canvasH = PAD * 2 + sortedLayerKeys.length * NODE_H +
                  (sortedLayerKeys.length - 1) * ROW_GAP;

  const pos = new Map(); // id -> { x, y } (top-left of node box)
  sortedLayerKeys.forEach((lk, rowIdx) => {
    const ids = layers.get(lk);
    const rowW = ids.length * NODE_W + (ids.length - 1) * COL_GAP;
    const startX = (canvasW - rowW) / 2;
    const y = PAD + rowIdx * (NODE_H + ROW_GAP);
    ids.forEach((id, colIdx) => {
      pos.set(id, { x: startX + colIdx * (NODE_W + COL_GAP), y });
    });
  });

  const positionedNodes = monitors.map(m => ({
    ...m,
    ...pos.get(m.id),
  }));

  // Edge endpoints: bottom-center of parent → top-center of child. Edges that
  // go "upward" (child above parent, e.g. from a cycle) still render — we just
  // anchor at the nearest sensible side so arrows remain visible.
  const positionedEdges = edges
    .filter(e => pos.has(e.from) && pos.has(e.to))
    .map(e => {
      const p = pos.get(e.from);
      const c = pos.get(e.to);
      const downward = c.y >= p.y;
      return {
        ...e,
        x1: p.x + NODE_W / 2,
        y1: downward ? p.y + NODE_H : p.y,
        x2: c.x + NODE_W / 2,
        y2: downward ? c.y : c.y + NODE_H,
        parentName: byId.get(e.from)?.name,
        childName: byId.get(e.to)?.name,
      };
    });

  return {
    nodes: positionedNodes,
    edges: positionedEdges,
    width: Math.max(canvasW, 320),
    height: Math.max(canvasH, 160),
  };
}

// Fan dependencies.list across every monitor and assemble a deduped edge set.
// `dependencies.list(child)` → { parents, children }. We take `parents`: each
// parent silences this child, so the outage-propagation edge is parent → child.
async function fetchGraph(signal) {
  const monitors = await api.monitors.list();
  const list = Array.isArray(monitors) ? monitors : [];
  const valid = new Set(list.map(m => m.id));

  const results = await Promise.all(
    list.map(async (m) => {
      try {
        const dep = await api.dependencies.list(m.id);
        return { child: m.id, parents: dep?.parents || [] };
      } catch {
        // A single monitor's dep fetch failing shouldn't kill the whole graph.
        return { child: m.id, parents: [] };
      }
    }),
  );

  const seen = new Set();
  const edges = [];
  for (const { child, parents } of results) {
    for (const parent of parents) {
      if (!valid.has(parent) || !valid.has(child)) continue;
      const key = `${parent}->${child}`;
      if (seen.has(key)) continue;
      seen.add(key);
      edges.push({ from: parent, to: child });
    }
  }

  return {
    nodes: list.map(m => ({ id: m.id, name: m.name, status: m.current_status })),
    edges,
  };
}

function LegendDot({ status, label }) {
  const c = statusColors(status);
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 12, color: 'var(--text-2)' }}>
      <span className="dep-legend-dot" style={{ background: c.fill, border: `1.5px solid ${c.stroke}` }} />
      {label}
    </span>
  );
}

export default function DependencyGraph() {
  const { data, error, loading } = useApi(fetchGraph, [], { pollMs: 15000 });
  const [hovered, setHovered] = useState(null);

  // Only lay out monitors that actually participate in a dependency edge —
  // dumping every edge-less monitor into the graph buries the real structure in
  // a wall of disconnected boxes. Orphans are counted and noted instead.
  const { layout, orphanCount } = useMemo(() => {
    if (!data || !data.nodes?.length) return { layout: null, orphanCount: 0 };
    const edges = data.edges || [];
    const connected = new Set(edges.flatMap(e => [e.from, e.to]));
    const nodes = data.nodes.filter(n => connected.has(n.id));
    return {
      layout: nodes.length ? buildLayout(nodes, edges) : null,
      orphanCount: data.nodes.length - nodes.length,
    };
  }, [data]);

  const goToMonitor = (id) => { window.location.hash = `#/monitor/${id}`; };

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 1200, margin: '0 auto', padding: '28px 24px 64px' }}>
        <SubViewHeader title={t('deps.title')} icon={Share2} />

        <div style={{ display: 'flex', alignItems: 'flex-start', gap: 12, marginBottom: 6 }}>
          <Share2 size={22} style={{ color: 'var(--accent)', marginTop: 4 }} />
          <div>
            <h1 style={{ margin: 0, fontSize: 22, fontWeight: 700 }}>{t('deps.title')}</h1>
            <p style={{ margin: '4px 0 0', fontSize: 13.5, color: 'var(--text-2)', maxWidth: 640 }}>
              {t('deps.subtitle')}
            </p>
          </div>
        </div>

        {/* legend + stats */}
        {data && data.nodes?.length > 0 && (
          <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 16, margin: '18px 0 14px' }}>
            <LegendDot status="up"      label={t('deps.legend.up')} />
            <LegendDot status="down"    label={t('deps.legend.down')} />
            <LegendDot status="warn"    label={t('deps.legend.warn')} />
            <LegendDot status="paused"  label={t('deps.legend.paused')} />
            <span style={{ flex: 1 }} />
            <span className="mono" style={{ fontSize: 12, color: 'var(--text-3)' }}>
              {t('deps.stats.monitors', { n: data.nodes.length })} · {t('deps.stats.edges', { n: (data.edges || []).length })}
              {orphanCount > 0 && ` · ${orphanCount} with no dependencies`}
            </span>
          </div>
        )}

        {loading && !data && (
          <div className="card" style={{ padding: 40, display: 'flex', alignItems: 'center', gap: 10, color: 'var(--text-2)', fontSize: 13 }}>
            <Loader2 size={16} className="spin" style={{ animation: 'spin 1s linear infinite' }} /> {t('deps.loading')}
          </div>
        )}

        {error && (
          <div className="banner-err" style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <AlertCircle size={16} /> {t('deps.error')}
          </div>
        )}

        {data && data.nodes?.length === 0 && !loading && (
          <div className="card" style={{ padding: '48px 32px', textAlign: 'center' }}>
            <h3 style={{ margin: '0 0 6px', fontSize: 15 }}>{t('deps.empty.title')}</h3>
            <p style={{ margin: 0, fontSize: 13, color: 'var(--text-2)' }}>{t('deps.empty.body')}</p>
          </div>
        )}

        {data && data.nodes?.length > 0 && (data.edges || []).length === 0 && (
          <div className="card" style={{ padding: '32px', textAlign: 'center', marginBottom: 16 }}>
            <h3 style={{ margin: '0 0 6px', fontSize: 15 }}>{t('deps.empty.title')}</h3>
            <p style={{ margin: 0, fontSize: 13, color: 'var(--text-2)' }}>{t('deps.empty.body')}</p>
          </div>
        )}

        {layout && (data.edges || []).length > 0 && (
          <div className="card" style={{ padding: 16, overflowX: 'auto' }}>
            <svg
              width={layout.width}
              height={layout.height}
              viewBox={`0 0 ${layout.width} ${layout.height}`}
              role="img"
              aria-label={t('deps.title')}
              style={{ display: 'block', maxWidth: '100%' }}
            >
              <defs>
                <marker id="dep-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                  <path d="M0,0 L10,5 L0,10 z" fill="var(--text-3)" />
                </marker>
              </defs>

              {/* edges first so nodes paint on top */}
              {layout.edges.map((e, i) => {
                const active = hovered && (e.from === hovered || e.to === hovered);
                return (
                  <line
                    key={`e-${i}`}
                    x1={e.x1} y1={e.y1} x2={e.x2} y2={e.y2}
                    stroke={active ? 'var(--accent)' : 'var(--text-3)'}
                    strokeWidth={active ? 2 : 1.4}
                    markerEnd="url(#dep-arrow)"
                    opacity={hovered && !active ? 0.25 : 0.85}
                  />
                );
              })}

              {/* nodes */}
              {layout.nodes.map((n) => {
                const c = statusColors(n.status);
                const dim = hovered && hovered !== n.id &&
                  !layout.edges.some(e => (e.from === hovered && e.to === n.id) || (e.to === hovered && e.from === n.id));
                return (
                  <g
                    key={n.id}
                    className="dep-node"
                    transform={`translate(${n.x}, ${n.y})`}
                    onClick={() => goToMonitor(n.id)}
                    onMouseEnter={() => setHovered(n.id)}
                    onMouseLeave={() => setHovered(null)}
                    opacity={dim ? 0.4 : 1}
                    role="button"
                    aria-label={t('deps.node.open', { name: n.name })}
                    tabIndex={0}
                    onKeyDown={(ev) => { if (ev.key === 'Enter' || ev.key === ' ') { ev.preventDefault(); goToMonitor(n.id); } }}
                  >
                    <rect
                      className="dep-node-box"
                      width={NODE_W} height={NODE_H} rx={10}
                      fill={c.fill} stroke={c.stroke} strokeWidth={1.5}
                    />
                    <circle cx={16} cy={NODE_H / 2} r={5} fill={c.stroke} />
                    <text
                      x={30} y={NODE_H / 2 + 4}
                      fontSize={13} fontWeight={600} fill={c.text}
                      style={{ pointerEvents: 'none' }}
                    >
                      {n.name?.length > 16 ? `${n.name.slice(0, 15)}…` : n.name}
                    </text>
                  </g>
                );
              })}
            </svg>
          </div>
        )}
      </div>
    </div>
  );
}
