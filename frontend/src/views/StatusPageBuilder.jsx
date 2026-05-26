import React, { useState } from 'react';
import {
  ChevronLeft, ChevronDown, ChevronRight, Eye, Code, Save, Globe,
  Plus, GripVertical, Trash2, Settings2, Palette, Type, Bell,
  CheckCircle2, AlertCircle, Smartphone, Monitor, Lock, Sparkles,
  Image as ImageIcon, ExternalLink, Copy, X
} from 'lucide-react';

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&family=Instrument+Serif:ital@0;1&display=swap');

  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --up:#10b981; --up-soft:#d1fae5;
    --down:#ef4444; --down-soft:#fee2e2;
    --warn:#f59e0b; --warn-soft:#fef3c7;

    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif;
    min-height: 100vh;
  }
  .rampart * { box-sizing: border-box; }
  .mono { font-family: 'JetBrains Mono', monospace; }
  .tabular { font-variant-numeric: tabular-nums; }

  .btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 7px 12px; border-radius: 8px; cursor: pointer;
    font-size: 13px; font-weight: 500; line-height: 1;
    background: var(--surface); border: 1px solid var(--border); color: var(--text-2);
    transition: all .12s;
  }
  .btn:hover { background: var(--surface-2); color: var(--text); border-color: var(--border-2); }
  .btn-prim { background: var(--text); color: var(--surface); border-color: var(--text); }
  .btn-accent { background: var(--accent); color: white; border-color: var(--accent); }
  .btn-accent:hover { background: var(--accent-2); }
  .btn-ghost { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }

  .field-label { font-size: 11px; font-weight: 500; color: var(--text-3); text-transform: uppercase; letter-spacing: .04em; margin-bottom: 6px; display: block; }
  .input {
    width: 100%; padding: 8px 11px; border-radius: 7px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 13px; color: var(--text); outline: none;
    font-family: inherit;
  }
  .input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }

  .section {
    border: 1px solid var(--border); border-radius: 10px;
    background: var(--surface); margin-bottom: 10px; overflow: hidden;
  }
  .section-head {
    display: flex; align-items: center; gap: 10px;
    padding: 12px 14px; cursor: pointer; user-select: none;
  }
  .section-head:hover { background: var(--surface-2); }
  .section-body { padding: 0 14px 14px; border-top: 1px solid var(--border); padding-top: 14px; }

  .swatch { width: 28px; height: 28px; border-radius: 8px; cursor: pointer; border: 2px solid transparent; }
  .swatch.active { border-color: var(--text); transform: scale(1.1); }

  .toggle { width: 32px; height: 18px; border-radius: 9px; background: var(--border-2); position: relative; cursor: pointer; transition: background .15s; flex-shrink: 0; }
  .toggle::after { content:''; position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: 50%; background: white; transition: all .15s; box-shadow: 0 1px 2px rgba(0,0,0,.1); }
  .toggle.on { background: var(--accent); }
  .toggle.on::after { left: 16px; }

  /* ── preview pane (PUBLIC PAGE STYLE) ──
     Editorial / light. Slightly warmer feel via Instrument Serif for headlines.
     Keeps the dashboard aesthetic but elevates for public-facing branding. */
  .preview {
    background: white; color: #18181b;
    border-radius: 12px; overflow: hidden;
    box-shadow: 0 20px 60px rgba(0,0,0,.08), 0 0 0 1px var(--border);
    font-family: Inter, sans-serif;
  }
  .preview * { box-sizing: border-box; }
  .pv-head {
    padding: 28px 36px 22px; border-bottom: 1px solid #f1f1ef;
    display: flex; align-items: center; justify-content: space-between;
  }
  .pv-logo { display: flex; align-items: center; gap: 10px; }
  .pv-logo .mark {
    width: 24px; height: 24px; border-radius: 7px;
    background: linear-gradient(135deg, #14b8a6, #0d9488);
    display: flex; align-items: center; justify-content: center;
    color: white; font-weight: 700; font-size: 13px;
  }
  .pv-brand { font-family: 'Instrument Serif', serif; font-size: 22px; }
  .pv-hero { padding: 60px 36px 44px; text-align: center; background:
    radial-gradient(ellipse at top, rgba(16,185,129,.05), transparent 70%); }
  .pv-hero h1 {
    font-family: 'Instrument Serif', serif;
    font-size: 44px; font-weight: 400; letter-spacing: -.015em;
    margin: 0 0 8px; line-height: 1.1; color: #18181b;
  }
  .pv-hero .sub { font-size: 13px; color: #6b6b6b; }
  .pv-incident {
    margin: 20px 36px 0; padding: 16px 18px;
    background: #fef3c7; border: 1px solid #fde68a;
    border-radius: 10px;
  }
  .pv-incident .lbl { font-size: 11px; font-weight: 600; color: #92400e; text-transform: uppercase; letter-spacing: .04em; margin-bottom: 6px; }
  .pv-incident h4 { font-family: 'Instrument Serif', serif; font-size: 18px; margin: 0 0 6px; color: #18181b; font-weight: 400; }
  .pv-incident p { font-size: 13px; color: #57534e; margin: 0; line-height: 1.55; }

  .pv-group { margin: 0 36px; padding: 20px 0; border-bottom: 1px solid #f1f1ef; }
  .pv-group h3 {
    font-family: 'Instrument Serif', serif; font-size: 18px; font-weight: 400;
    color: #18181b; margin: 0 0 14px;
    display: flex; align-items: baseline; justify-content: space-between;
  }
  .pv-group h3 small {
    font-family: 'JetBrains Mono', monospace; font-size: 10px; color: #8a8a8a;
    letter-spacing: .1em; text-transform: uppercase;
  }
  .pv-comp { padding: 12px 0; border-top: 1px solid #f5f5f4; }
  .pv-comp:first-child { border-top: none; }
  .pv-comp-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 8px; }
  .pv-comp-name { display: flex; align-items: center; gap: 10px; font-size: 14px; font-weight: 500; }
  .pv-comp-status { font-family: 'JetBrains Mono', monospace; font-size: 10px; letter-spacing: .06em; text-transform: uppercase; padding: 2px 8px; border-radius: 4px; }
  .pv-comp-status.up   { background: #d1fae5; color: #047857; }
  .pv-comp-status.warn { background: #fef3c7; color: #92400e; }
  .pv-comp-status.down { background: #fee2e2; color: #b91c1c; }
  .pv-bars { display: flex; gap: 2px; height: 26px; }
  .pv-bars > div { flex: 1; border-radius: 2px; min-width: 2px; }
  .pv-bars .up   { background: #6ee7b7; }
  .pv-bars .up2  { background: #34d399; }
  .pv-bars .warn { background: #fbbf24; }
  .pv-bars .down { background: #ef4444; }
  .pv-bars-foot { display: flex; justify-content: space-between; font-family: 'JetBrains Mono', monospace; font-size: 10px; color: #8a8a8a; margin-top: 6px; }

  .pv-sub {
    margin: 32px 36px; padding: 28px;
    background: #18181b; color: white; border-radius: 12px; text-align: center;
  }
  .pv-sub h3 { font-family: 'Instrument Serif', serif; font-size: 24px; font-weight: 400; margin: 0 0 6px; }
  .pv-sub p { font-size: 13px; color: #a1a1aa; margin: 0 0 16px; }
  .pv-sub .row { display: flex; gap: 6px; max-width: 380px; margin: 0 auto; }
  .pv-sub input { flex: 1; background: rgba(255,255,255,.05); border: 1px solid rgba(255,255,255,.1); color: white; padding: 9px 12px; border-radius: 7px; font-size: 13px; outline: none; font-family: inherit; }
  .pv-sub button { background: var(--accent); color: white; border: none; padding: 9px 18px; font-size: 13px; font-weight: 500; border-radius: 7px; cursor: pointer; }
  .pv-foot { padding: 22px 36px; text-align: center; font-size: 11px; color: #8a8a8a; border-top: 1px solid #f1f1ef; }
`;

// ── seed data ─────────────────────────────────────────────────────────────
const groups = [
  { name: 'Public APIs',  components: [
    { name: 'api.example.com', status: 'up',   uptime: 99.99 },
    { name: 'auth service',    status: 'up',   uptime: 99.98 },
    { name: 'cdn',             status: 'up',   uptime: 100   },
  ]},
  { name: 'Payments',     components: [
    { name: 'payments gateway', status: 'down', uptime: 97.21 },
    { name: 'webhooks',         status: 'warn', uptime: 99.4  },
    { name: 'invoicing',        status: 'up',   uptime: 99.91 },
  ]},
  { name: 'Dashboard',    components: [
    { name: 'app.example.com', status: 'up', uptime: 99.94 },
    { name: 'realtime ws',     status: 'up', uptime: 99.88 },
  ]},
];

const seed = (s) => { let x = s; return () => (x = (x*9301+49297)%233280) / 233280; };
const bars90 = (status, seedN) => {
  const r = seed(seedN);
  return Array.from({ length: 90 }, (_, i) => {
    if (status === 'down' && i > 86) return 'down';
    if (status === 'warn' && i > 80 && r() < .4) return 'warn';
    if (r() < 0.01) return 'down';
    if (r() < 0.02) return 'warn';
    return i % 5 === 0 ? 'up2' : 'up';
  });
};

// ── main ──────────────────────────────────────────────────────────────────
export default function StatusPageBuilder() {
  const [device, setDevice]   = useState('desktop');
  const [accent, setAccent]   = useState('#14b8a6');
  const [openSections, setOpenSections] = useState({
    brand: true, components: true, theme: false, sub: false, domain: false
  });

  return (
    <div className="rampart">
      <style>{css}</style>

      {/* top bar */}
      <header style={{
        display: 'flex', alignItems: 'center', gap: 16,
        padding: '12px 20px', borderBottom: '1px solid var(--border)',
        background: 'var(--surface)', position: 'sticky', top: 0, zIndex: 10
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <ChevronLeft size={16} color="var(--text-2)" style={{ cursor: 'pointer' }}/>
          <span style={{ fontSize: 14, color: 'var(--text-3)' }}>Status pages /</span>
          <span style={{ fontSize: 14, fontWeight: 500 }}>example.com</span>
        </div>

        <div style={{ display: 'flex', gap: 4, marginLeft: 20, alignItems: 'center' }}>
          <button className="btn" style={{ background: device === 'desktop' ? 'var(--surface-2)' : 'transparent' }} onClick={() => setDevice('desktop')}>
            <Monitor size={13}/>
          </button>
          <button className="btn" style={{ background: device === 'mobile' ? 'var(--surface-2)' : 'transparent' }} onClick={() => setDevice('mobile')}>
            <Smartphone size={13}/>
          </button>
        </div>

        <div style={{ marginLeft: 'auto', display: 'flex', gap: 8, alignItems: 'center' }}>
          <span style={{ fontSize: 12, color: 'var(--text-3)', display: 'flex', alignItems: 'center', gap: 6 }}>
            <CheckCircle2 size={12} color="var(--up)"/>
            status.example.com · auto-saved 12s ago
          </span>
          <button className="btn"><Code size={13}/> Embed</button>
          <button className="btn"><ExternalLink size={13}/> View</button>
          <button className="btn btn-accent"><Save size={13}/> Publish</button>
        </div>
      </header>

      <div style={{ display: 'grid', gridTemplateColumns: '380px 1fr', height: 'calc(100vh - 53px)' }}>

        {/* ─── EDITOR ───────────────────────────────────────────── */}
        <aside style={{ borderRight: '1px solid var(--border)', background: 'var(--surface)', padding: '20px 16px', overflowY: 'auto' }}>
          <div style={{ marginBottom: 18 }}>
            <h2 style={{ fontSize: 18, fontWeight: 600, margin: '0 0 6px', letterSpacing: '-.01em' }}>Status page</h2>
            <p style={{ fontSize: 12, color: 'var(--text-3)', margin: 0 }}>Drag sections to reorder · auto-saves</p>
          </div>

          {/* Brand */}
          <div className="section">
            <div className="section-head" onClick={() => setOpenSections(s => ({ ...s, brand: !s.brand }))}>
              <GripVertical size={13} color="var(--text-3)"/>
              <Type size={13} color="var(--text-2)"/>
              <span style={{ flex: 1, fontSize: 13, fontWeight: 500 }}>Branding</span>
              {openSections.brand ? <ChevronDown size={13} color="var(--text-3)"/> : <ChevronRight size={13} color="var(--text-3)"/>}
            </div>
            {openSections.brand && (
              <div className="section-body">
                <div style={{ marginBottom: 12 }}>
                  <label className="field-label">Title</label>
                  <input className="input" defaultValue="Example Status"/>
                </div>
                <div style={{ marginBottom: 12 }}>
                  <label className="field-label">Subtitle</label>
                  <input className="input" defaultValue="Real-time service status"/>
                </div>
                <div>
                  <label className="field-label">Icon / logo</label>
                  <button className="btn" style={{ width: '100%', justifyContent: 'center' }}>
                    <ImageIcon size={13}/> Upload SVG or PNG
                  </button>
                </div>
              </div>
            )}
          </div>

          {/* Components */}
          <div className="section">
            <div className="section-head" onClick={() => setOpenSections(s => ({ ...s, components: !s.components }))}>
              <GripVertical size={13} color="var(--text-3)"/>
              <Settings2 size={13} color="var(--text-2)"/>
              <span style={{ flex: 1, fontSize: 13, fontWeight: 500 }}>Components</span>
              <span className="mono" style={{ fontSize: 11, color: 'var(--text-3)' }}>8</span>
              {openSections.components ? <ChevronDown size={13} color="var(--text-3)"/> : <ChevronRight size={13} color="var(--text-3)"/>}
            </div>
            {openSections.components && (
              <div className="section-body">
                {groups.map(g => (
                  <div key={g.name} style={{ marginBottom: 14 }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 6, fontSize: 11, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em', fontWeight: 600 }}>
                      <ChevronDown size={11}/>
                      <span style={{ flex: 1 }}>{g.name}</span>
                      <Plus size={11} style={{ cursor: 'pointer' }}/>
                    </div>
                    {g.components.map((c, i) => (
                      <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '5px 0 5px 17px' }}>
                        <span style={{ width: 7, height: 7, borderRadius: '50%', background: c.status === 'up' ? 'var(--up)' : c.status === 'warn' ? 'var(--warn)' : 'var(--down)' }}/>
                        <span style={{ fontSize: 12.5, flex: 1 }}>{c.name}</span>
                        <X size={11} color="var(--text-3)" style={{ cursor: 'pointer', opacity: .6 }}/>
                      </div>
                    ))}
                  </div>
                ))}
                <button className="btn" style={{ width: '100%', justifyContent: 'center' }}>
                  <Plus size={12}/> Add group
                </button>
              </div>
            )}
          </div>

          {/* Theme */}
          <div className="section">
            <div className="section-head" onClick={() => setOpenSections(s => ({ ...s, theme: !s.theme }))}>
              <GripVertical size={13} color="var(--text-3)"/>
              <Palette size={13} color="var(--text-2)"/>
              <span style={{ flex: 1, fontSize: 13, fontWeight: 500 }}>Theme</span>
              {openSections.theme ? <ChevronDown size={13} color="var(--text-3)"/> : <ChevronRight size={13} color="var(--text-3)"/>}
            </div>
            {openSections.theme && (
              <div className="section-body">
                <div style={{ marginBottom: 14 }}>
                  <label className="field-label">Accent color</label>
                  <div style={{ display: 'flex', gap: 8 }}>
                    {['#14b8a6','#6366f1','#ec4899','#f59e0b','#10b981','#0ea5e9'].map(c => (
                      <div key={c} className={`swatch ${accent === c ? 'active' : ''}`} style={{ background: c }} onClick={() => setAccent(c)}/>
                    ))}
                  </div>
                </div>
                <div>
                  <label className="field-label">Mode</label>
                  <div style={{ display: 'flex', gap: 6 }}>
                    {['Light','Dark','Auto'].map((m, i) => (
                      <button key={m} className="btn" style={{
                        flex: 1, justifyContent: 'center',
                        background: i === 0 ? 'var(--surface-2)' : 'transparent',
                        borderColor: i === 0 ? 'var(--border-2)' : 'var(--border)',
                        color: i === 0 ? 'var(--text)' : 'var(--text-2)',
                      }}>{m}</button>
                    ))}
                  </div>
                </div>
              </div>
            )}
          </div>

          {/* Subscribers */}
          <div className="section">
            <div className="section-head" onClick={() => setOpenSections(s => ({ ...s, sub: !s.sub }))}>
              <GripVertical size={13} color="var(--text-3)"/>
              <Bell size={13} color="var(--text-2)"/>
              <span style={{ flex: 1, fontSize: 13, fontWeight: 500 }}>Subscribers</span>
              <span className="mono" style={{ fontSize: 11, color: 'var(--text-3)' }}>1,247</span>
              {openSections.sub ? <ChevronDown size={13} color="var(--text-3)"/> : <ChevronRight size={13} color="var(--text-3)"/>}
            </div>
            {openSections.sub && (
              <div className="section-body">
                <label className="field-label">Allow subscriptions via</label>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                  {['Email','SMS','Slack webhook','RSS','Atom'].map((c, i) => (
                    <label key={c} style={{ display: 'flex', alignItems: 'center', gap: 10, cursor: 'pointer', fontSize: 13 }}>
                      <div className={`toggle ${i !== 1 ? 'on' : ''}`}/>
                      <span style={{ color: 'var(--text-2)' }}>{c}</span>
                    </label>
                  ))}
                </div>
              </div>
            )}
          </div>

          {/* Domain */}
          <div className="section">
            <div className="section-head" onClick={() => setOpenSections(s => ({ ...s, domain: !s.domain }))}>
              <GripVertical size={13} color="var(--text-3)"/>
              <Globe size={13} color="var(--text-2)"/>
              <span style={{ flex: 1, fontSize: 13, fontWeight: 500 }}>Domain</span>
              {openSections.domain ? <ChevronDown size={13} color="var(--text-3)"/> : <ChevronRight size={13} color="var(--text-3)"/>}
            </div>
            {openSections.domain && (
              <div className="section-body">
                <label className="field-label">Custom domain</label>
                <input className="input" defaultValue="status.example.com"/>
                <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginTop: 8, fontSize: 12, color: 'var(--up)' }}>
                  <CheckCircle2 size={12}/> Verified · TLS auto-renewing
                </div>
              </div>
            )}
          </div>

          <button className="btn" style={{ width: '100%', justifyContent: 'center', marginTop: 12 }}>
            <Plus size={12}/> Add section
          </button>
        </aside>

        {/* ─── PREVIEW ───────────────────────────────────────────── */}
        <main style={{ padding: 32, overflowY: 'auto' }}>
          <div style={{ maxWidth: device === 'mobile' ? 380 : 880, margin: '0 auto', transition: 'max-width .3s' }}>
            <div className="preview">
              {/* header */}
              <div className="pv-head">
                <div className="pv-logo">
                  <div className="mark">B</div>
                  <span className="pv-brand">Example Status</span>
                </div>
                <button style={{ background: 'transparent', border: '1px solid #e7e5e4', color: '#18181b', padding: '6px 12px', borderRadius: 7, fontSize: 12, cursor: 'pointer', fontFamily: 'inherit', fontWeight: 500 }}>Subscribe</button>
              </div>

              {/* hero */}
              <div className="pv-hero">
                <h1>Some services are <em style={{ color: '#b45309', fontStyle: 'italic' }}>experiencing issues</em>.</h1>
                <div className="sub">Updated 14 seconds ago · investigating</div>
              </div>

              {/* active incident */}
              <div className="pv-incident">
                <div className="lbl">● Investigating · 4 min ago</div>
                <h4>Payments are timing out for some customers</h4>
                <p>We're seeing elevated error rates on our payments gateway. The team is investigating an upstream provider issue. We'll post an update within 15 minutes.</p>
              </div>

              {/* component groups */}
              {groups.map((g, gi) => (
                <div key={gi} className="pv-group">
                  <h3>
                    {g.name}
                    <small>{g.components.filter(c => c.status === 'up').length}/{g.components.length} operational</small>
                  </h3>
                  {g.components.map((c, ci) => {
                    const bars = bars90(c.status, gi * 10 + ci);
                    return (
                      <div key={ci} className="pv-comp">
                        <div className="pv-comp-head">
                          <div className="pv-comp-name">
                            <span style={{ width: 8, height: 8, borderRadius: '50%', background: c.status === 'up' ? '#10b981' : c.status === 'warn' ? '#f59e0b' : '#ef4444' }}/>
                            {c.name}
                          </div>
                          <span className={`pv-comp-status ${c.status}`}>
                            {c.status === 'up' ? 'Operational' : c.status === 'warn' ? 'Degraded' : 'Outage'}
                          </span>
                        </div>
                        <div className="pv-bars">{bars.map((b, i) => <div key={i} className={b}/>)}</div>
                        <div className="pv-bars-foot">
                          <span>90 days ago</span>
                          <span>{c.uptime}% uptime</span>
                          <span>today</span>
                        </div>
                      </div>
                    );
                  })}
                </div>
              ))}

              {/* subscribe */}
              <div className="pv-sub">
                <h3>Get notified before your customers ask.</h3>
                <p>Subscribe to receive status updates. Unsubscribe anytime.</p>
                <div className="row">
                  <input placeholder="you@company.com"/>
                  <button>Subscribe</button>
                </div>
              </div>

              <div className="pv-foot">
                Powered by <strong style={{ color: '#18181b' }}>Rampart</strong> · last 90 days · all times UTC
              </div>
            </div>

            <div style={{ marginTop: 14, textAlign: 'center', fontSize: 11, color: 'var(--text-3)' }}>
              live preview · changes save automatically
            </div>
          </div>
        </main>
      </div>
    </div>
  );
}
