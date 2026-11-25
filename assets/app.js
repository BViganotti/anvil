'use strict';

// Anvil Studio client. Original code, no dependencies. The token is created by
// the launcher and passed in the URL; API requests must carry it.

const $ = (id) => document.getElementById(id);
const token = (() => {
  const p = new URLSearchParams(location.search);
  const t = p.get('token');
  if (t) { try { sessionStorage.setItem('anvil-token', t); } catch (_) {} return t; }
  try { return sessionStorage.getItem('anvil-token') || ''; } catch (_) { return ''; }
})();

const NM = 1e6; // nanometres per millimetre
const GRID = 1270000;
const COLORS = ['#7ee787', '#79c0ff', '#ffa657', '#ff7b72', '#d2a8ff', '#56d4dd', '#f2cc60', '#ff9bce'];

const S = {
  state: null,
  lib: null,
  board: null,
  drc: null,
  tab: 'schematic',
  sel: null,
  tool: 'select',
  pendingPart: null,
  wireStart: null,
  mouse: { x: 0, y: 0, world: [0, 0] },
  sch: { cx: 0, cy: 0, scale: 0.000006, panX: 0, panY: 0, fitted: false },
  bd: { cx: 0, cy: 0, scale: 0.000006, panX: 0, panY: 0, fitted: false },
  drag: null,
  pan: null,
  plot: null,
  routeES: null,
  noticeTimer: 0,
};

function esc(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

function notify(msg, bad) {
  const el = $('toast');
  el.textContent = String(msg);
  el.className = 'toast show' + (bad ? ' bad' : '');
  clearTimeout(S.noticeTimer);
  S.noticeTimer = setTimeout(() => { el.className = 'toast'; }, bad ? 6000 : 3000);
}

async function api(path, body, method) {
  const opts = {
    method: method || (body === undefined ? 'GET' : 'POST'),
    headers: { 'X-Anvil-Token': token },
  };
  if (body !== undefined) {
    opts.headers['Content-Type'] = 'application/json';
    opts.body = JSON.stringify(body);
  }
  const res = await fetch(path, opts);
  if (!res.ok) {
    let message = res.status + ' ' + res.statusText;
    try {
      const j = await res.json();
      if (j && j.error) message = j.error;
    } catch (_) {
      try { const t = await res.text(); if (t) message = t.slice(0, 300); } catch (_) {}
    }
    throw new Error(message);
  }
  return res;
}

async function apiJson(path, body) {
  return (await api(path, body)).json();
}

function setStatus(id, text, kind) {
  const el = $(id);
  if (!el) return;
  el.textContent = text;
  el.className = 'pill' + (kind ? ' ' + kind : '');
}

// ---------------------------------------------------------------- canvases

function canvasSize(canvas) {
  const r = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(1, Math.round(r.width));
  const h = Math.max(1, Math.round(r.height));
  if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
  }
  const ctx = canvas.getContext('2d');
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  return { ctx, w, h };
}

function schScreen(x, y, w, h) {
  const v = S.sch;
  return [w / 2 + v.panX + (x - v.cx) * v.scale, h / 2 + v.panY - (y - v.cy) * v.scale];
}

function schWorld(sx, sy, w, h) {
  const v = S.sch;
  return [v.cx + (sx - w / 2 - v.panX) / v.scale, v.cy - (sy - h / 2 - v.panY) / v.scale];
}

function bdScreen(x, y, w, h) {
  const v = S.bd;
  return [w / 2 + v.panX + (x - v.cx) * v.scale, h / 2 + v.panY - (y - v.cy) * v.scale];
}

function bdWorld(sx, sy, w, h) {
  const v = S.bd;
  return [v.cx + (sx - w / 2 - v.panX) / v.scale, v.cy - (sy - h / 2 - v.panY) / v.scale];
}

function fitView(kind) {
  const isSch = kind === 'sch';
  const v = isSch ? S.sch : S.bd;
  let b = null;
  if (isSch) {
    b = S.state && S.state.schematic ? S.state.schematic.bounds : null;
  } else if (S.board) {
    b = S.board.bounds;
  }
  if (!b) return;
  const canvas = isSch ? $('schematic-canvas') : $('board-canvas');
  const { w, h } = canvasSize(canvas);
  const bw = Math.max(1, b[2] - b[0]);
  const bh = Math.max(1, b[3] - b[1]);
  const scale = Math.min((w - 90) / bw, (h - 90) / bh);
  v.cx = (b[0] + b[2]) / 2;
  v.cy = (b[1] + b[3]) / 2;
  v.scale = Math.max(1e-7, scale);
  v.panX = 0;
  v.panY = 0;
  v.fitted = true;
}

// ---------------------------------------------------------------- schematic

function localPoint(c, x, y) {
  if (c.mirror) x = -x;
  const rot = ((c.rotation % 360) + 360) % 360;
  if (rot === 90) { const t = x; x = y; y = -t; }
  else if (rot === 180) { x = -x; y = -y; }
  else if (rot === 270) { const t = x; x = -y; y = t; }
  return [x, y];
}

function recomputePins(c) {
  for (const p of c.pins) {
    const [x, y] = localPoint(c, p.offset[0], p.offset[1]);
    p.at = [c.at[0] + x, c.at[1] + y];
  }
}

function symbolFor(name) {
  if (!S.lib) return null;
  return (S.lib.symbols || []).find((s) => s.name === name) || null;
}

function drawSchematic() {
  const canvas = $('schematic-canvas');
  if (!canvas || S.tab !== 'schematic') return;
  const { ctx, w, h } = canvasSize(canvas);
  if (!S.state) return;
  if (!S.sch.fitted) fitView('sch');
  ctx.clearRect(0, 0, w, h);
  ctx.fillStyle = '#0a0f16';
  ctx.fillRect(0, 0, w, h);

  // Grid.
  const v = S.sch;
  const step = 5 * GRID;
  const [x0, y1] = schWorld(0, 0, w, h);
  const [x1, y0] = schWorld(w, h, w, h);
  ctx.strokeStyle = '#131d29';
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let x = Math.floor(x0 / step) * step; x <= x1; x += step) {
    const [sx] = schScreen(x, 0, w, h);
    ctx.moveTo(sx, 0); ctx.lineTo(sx, h);
  }
  for (let y = Math.floor(y0 / step) * step; y <= y1; y += step) {
    const [, sy] = schScreen(0, y, w, h);
    ctx.moveTo(0, sy); ctx.lineTo(w, sy);
  }
  ctx.stroke();

  const sch = S.state.schematic;
  // Wires.
  for (const wire of sch.wires || []) {
    ctx.strokeStyle = S.sel && S.sel.kind === 'wire' && S.sel.id === wire.id ? '#f2cc60' : '#3fb950';
    ctx.lineWidth = S.sel && S.sel.kind === 'wire' && S.sel.id === wire.id ? 3 : 2;
    ctx.beginPath();
    wire.points.forEach((p, i) => {
      const [sx, sy] = schScreen(p[0], p[1], w, h);
      if (i === 0) ctx.moveTo(sx, sy); else ctx.lineTo(sx, sy);
    });
    ctx.stroke();
  }
  // Junctions.
  for (const j of sch.junctions || []) {
    const [sx, sy] = schScreen(j.at[0], j.at[1], w, h);
    ctx.fillStyle = '#3fb950';
    ctx.beginPath(); ctx.arc(sx, sy, 3, 0, Math.PI * 2); ctx.fill();
  }
  // Components.
  const comps = sch.components || [];
  for (const c of comps) {
    const sym = symbolFor(c.symbol);
    const selected = S.sel && S.sel.kind === 'component' && S.sel.id === c.id;
    ctx.save();
    if (selected) {
      const [sx, sy] = schScreen(c.at[0], c.at[1], w, h);
      ctx.strokeStyle = '#58a6ff';
      ctx.lineWidth = 1.5;
      ctx.setLineDash([4, 3]);
      const rr = Math.max(26, (sym ? Math.max(sym.body[0], sym.body[1]) : 2 * GRID) * v.scale * 0.9);
      ctx.strokeRect(sx - rr, sy - rr, rr * 2, rr * 2);
      ctx.setLineDash([]);
    }
    ctx.strokeStyle = selected ? '#79c0ff' : '#c9d1d9';
    ctx.fillStyle = '#0a0f16';
    ctx.lineWidth = 1.6;
    if (sym) {
      for (const g of sym.graphics || []) {
        drawGraphic(ctx, g, c, w, h);
      }
      for (const p of sym.pins || []) {
        const [ox, oy] = localPoint(c, p.at[0], p.at[1]);
        const [px, py] = schScreen(c.at[0] + ox, c.at[1] + oy, w, h);
        ctx.fillStyle = '#8b949e';
        ctx.beginPath(); ctx.arc(px, py, 2.5, 0, Math.PI * 2); ctx.fill();
      }
    }
    const [sx, sy] = schScreen(c.at[0], c.at[1], w, h);
    ctx.fillStyle = selected ? '#79c0ff' : '#e6edf3';
    ctx.font = '11px ui-monospace, Menlo, monospace';
    ctx.textAlign = 'center';
    ctx.fillText(c.refdes, sx, sy - 30);
    ctx.fillStyle = '#8b949e';
    ctx.fillText(c.value, sx, sy - 18);
    ctx.restore();
  }
  // Labels.
  for (const l of sch.labels || []) {
    const [sx, sy] = schScreen(l.at[0], l.at[1], w, h);
    ctx.font = '10px ui-monospace, Menlo, monospace';
    const tw = ctx.measureText(l.name).width + 8;
    ctx.fillStyle = l.kind === 'power' ? 'rgba(30,60,40,.92)' : 'rgba(24,36,52,.92)';
    ctx.strokeStyle = l.kind === 'power' ? '#2a7a45' : '#2e4b6b';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.roundRect(sx + 4, sy - 8, tw, 15, 3);
    ctx.fill(); ctx.stroke();
    ctx.fillStyle = l.kind === 'power' ? '#7ee787' : '#79c0ff';
    ctx.textAlign = 'left';
    ctx.fillText(l.name, sx + 8, sy + 3);
  }
  // Wire preview.
  if (S.tool === 'wire' && S.wireStart) {
    const [ax, ay] = schScreen(S.wireStart[0], S.wireStart[1], w, h);
    const [bx, by] = schScreen(S.mouse.world[0], S.mouse.world[1], w, h);
    ctx.strokeStyle = '#f2cc60';
    ctx.setLineDash([5, 4]);
    ctx.beginPath(); ctx.moveTo(ax, ay); ctx.lineTo(bx, by);
    ctx.stroke();
    ctx.setLineDash([]);
  }
  const [mx, my] = S.mouse.world;
  $('schematic-hud').textContent =
    `${(mx / NM).toFixed(2)}, ${(my / NM).toFixed(2)} mm  ·  ${(v.scale * NM).toFixed(1)} px/mm  ·  ${comps.length} components`;

  const d = S.state.netlist || { errors: 0, warnings: 0 };
  setStatus('net-status', d.errors ? `netlist: ${d.errors} errors` : (d.warnings ? `netlist: ${d.warnings} warnings` : 'netlist clean'), d.errors ? 'bad' : (d.warnings ? 'warn' : 'ok'));
}

function drawGraphic(ctx, g, c, w, h) {
  const T = (x, y) => {
    const [ox, oy] = localPoint(c, x, y);
    return schScreen(c.at[0] + ox, c.at[1] + oy, w, h);
  };
  ctx.beginPath();
  if (g.type === 'line') {
    const a = T(g.a[0], g.a[1]); const b = T(g.b[0], g.b[1]);
    ctx.moveTo(a[0], a[1]); ctx.lineTo(b[0], b[1]); ctx.stroke();
  } else if (g.type === 'rect') {
    const a = T(g.a[0], g.a[1]); const b = T(g.b[0], g.b[1]);
    ctx.rect(Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.abs(b[0] - a[0]), Math.abs(b[1] - a[1]));
    ctx.stroke();
  } else if (g.type === 'circle') {
    const a = T(g.at[0], g.at[1]);
    ctx.arc(a[0], a[1], Math.max(1, g.r * S.sch.scale), 0, Math.PI * 2);
    ctx.stroke();
  } else if (g.type === 'poly') {
    g.points.forEach((p, i) => { const q = T(p[0], p[1]); if (i === 0) ctx.moveTo(q[0], q[1]); else ctx.lineTo(q[0], q[1]); });
    ctx.stroke();
  } else if (g.type === 'text') {
    const a = T(g.at[0], g.at[1]);
    ctx.fillStyle = '#c9d1d9';
    ctx.font = `${Math.max(7, g.size * S.sch.scale)}px ui-monospace, Menlo, monospace`;
    ctx.textAlign = 'center';
    ctx.fillText(g.text, a[0], a[1]);
  }
}

function hitTestSchematic(sx, sy, w, h) {
  if (!S.state) return null;
  const comps = S.state.schematic.components || [];
  for (let i = comps.length - 1; i >= 0; i--) {
    const c = comps[i];
    const [cx, cy] = schScreen(c.at[0], c.at[1], w, h);
    let near = Math.hypot(sx - cx, sy - cy) < 32;
    for (const p of c.pins || []) {
      const [px, py] = schScreen(p.at[0], p.at[1], w, h);
      if (Math.hypot(sx - px, sy - py) < 12) near = true;
    }
    if (near) return { kind: 'component', id: c.id, comp: c };
  }
  for (const wire of (S.state.schematic.wires || []).slice().reverse()) {
    for (let i = 0; i + 1 < wire.points.length; i++) {
      const a = schScreen(wire.points[i][0], wire.points[i][1], w, h);
      const b = schScreen(wire.points[i + 1][0], wire.points[i + 1][1], w, h);
      if (distPointSeg(sx, sy, a[0], a[1], b[0], b[1]) < 7) return { kind: 'wire', id: wire.id, wire };
    }
  }
  return null;
}

function distPointSeg(px, py, ax, ay, bx, by) {
  const dx = bx - ax, dy = by - ay;
  const l2 = dx * dx + dy * dy;
  let t = l2 > 0 ? ((px - ax) * dx + (py - ay) * dy) / l2 : 0;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
}

// ---------------------------------------------------------------- board

function drawBoard() {
  const canvas = $('board-canvas');
  if (!canvas || S.tab !== 'board') return;
  const { ctx, w, h } = canvasSize(canvas);
  ctx.clearRect(0, 0, w, h);
  ctx.fillStyle = '#0a0f16'; ctx.fillRect(0, 0, w, h);
  if (!S.board) {
    ctx.fillStyle = '#8b949e'; ctx.font = '14px system-ui, sans-serif'; ctx.textAlign = 'center';
    ctx.fillText('No board. Generate one from the schematic.', w / 2, h / 2);
    return;
  }
  if (!S.bd.fitted) fitView('bd');
  const b = S.board;
  const v = S.bd;
  const [ox, oy] = bdScreen(0, 0, w, h);
  // Outline.
  if ((b.outline || []).length >= 3) {
    ctx.beginPath();
    b.outline.forEach((p, i) => { const q = bdScreen(p[0], p[1], w, h); if (i === 0) ctx.moveTo(q[0], q[1]); else ctx.lineTo(q[0], q[1]); });
    ctx.closePath();
    ctx.fillStyle = '#0e1a13';
    ctx.fill();
    ctx.strokeStyle = '#2a5b36'; ctx.lineWidth = 1.5; ctx.stroke();
  }
  // Ratsnest.
  if ($('show-rats').checked) {
    ctx.strokeStyle = 'rgba(242,204,96,.65)';
    ctx.setLineDash([4, 3]); ctx.lineWidth = 1;
    ctx.beginPath();
    for (const e of b.ratsnest || []) {
      const a = bdScreen(e.a[0], e.a[1], w, h);
      const c = bdScreen(e.b[0], e.b[1], w, h);
      ctx.moveTo(a[0], a[1]); ctx.lineTo(c[0], c[1]);
    }
    ctx.stroke(); ctx.setLineDash([]);
  }
  // Traces.
  for (const t of b.traces || []) {
    if (t.layer === 'bottom' && !$('show-bottom').checked) continue;
    if (t.layer === 'top' && !$('show-top').checked) continue;
    ctx.strokeStyle = t.layer === 'bottom' ? '#4d8fd0' : '#d9615c';
    ctx.lineWidth = Math.max(1.2, t.width * v.scale);
    ctx.lineCap = 'round';
    ctx.beginPath();
    t.points.forEach((p, i) => { const q = bdScreen(p[0], p[1], w, h); if (i === 0) ctx.moveTo(q[0], q[1]); else ctx.lineTo(q[0], q[1]); });
    ctx.stroke();
  }
  // Pads and vias.
  for (const f of b.footprints || []) {
    for (const p of f.pads || []) {
      const layerTop = p.layers !== 'bottom';
      if (!layerTop && !$('show-bottom').checked) continue;
      const [cx, cy] = bdScreen(p.at[0], p.at[1], w, h);
      const pw = Math.max(1.5, p.size[0] * v.scale);
      const ph = Math.max(1.5, p.size[1] * v.scale);
      ctx.fillStyle = p.drill > 0 ? '#c9a227' : (p.layers === 'bottom' ? '#4d8fd0' : '#d9615c');
      if (p.shape === 'circle') {
        ctx.beginPath(); ctx.arc(cx, cy, Math.max(pw, ph) / 2, 0, Math.PI * 2); ctx.fill();
      } else {
        ctx.fillRect(cx - pw / 2, cy - ph / 2, pw, ph);
      }
      if (p.drill > 0) {
        const dr = Math.max(1, p.drill * v.scale / 2);
        ctx.fillStyle = '#0a0f16';
        ctx.beginPath(); ctx.arc(cx, cy, dr, 0, Math.PI * 2); ctx.fill();
      }
    }
  }
  for (const vi of b.vias || []) {
    const [cx, cy] = bdScreen(vi.at[0], vi.at[1], w, h);
    const r = Math.max(1.5, vi.diameter * v.scale / 2);
    ctx.fillStyle = '#e3b341';
    ctx.beginPath(); ctx.arc(cx, cy, r, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = '#0a0f16';
    ctx.beginPath(); ctx.arc(cx, cy, Math.max(0.5, vi.drill * v.scale / 2), 0, Math.PI * 2); ctx.fill();
  }
  // Footprint refdes.
  ctx.fillStyle = '#8b949e';
  ctx.font = '10px ui-monospace, Menlo, monospace';
  ctx.textAlign = 'center';
  for (const f of b.footprints || []) {
    const [cx, cy] = bdScreen(f.at[0], f.at[1], w, h);
    ctx.fillText(f.refdes, cx, cy - 8);
  }
  // DRC markers.
  for (const vv of (S.drc && S.drc.violations) || []) {
    const [cx, cy] = bdScreen(vv.at[0], vv.at[1], w, h);
    ctx.strokeStyle = vv.severity === 'warning' ? '#d29922' : '#f85149';
    ctx.lineWidth = 1.5;
    ctx.beginPath(); ctx.arc(cx, cy, 6, 0, Math.PI * 2); ctx.stroke();
  }
  $('board-hud').textContent = `${b.footprints.length} footprints · ${b.traces.length} traces · ${b.vias.length} vias · ${(v.scale * NM).toFixed(1)} px/mm`;
}

// ---------------------------------------------------------------- panels

function renderState() {
  if (!S.state) return;
  $('project-name').textContent = S.state.name;
  $('sheet-title').textContent = S.state.sheet ? S.state.sheet.title : '';
  renderNetlist();
  populateExamples();
  renderDrcSummary();
  renderRouteStatus();
  setStatus('drc-status',
    S.state.drc ? (S.state.drc.clean ? 'DRC clean' : `DRC ${S.state.drc.errors}`) : 'DRC —',
    S.state.drc ? (S.state.drc.clean ? 'ok' : 'bad') : '');
  if (S.state.board && !S.state.board.present) setStatus('route-status', 'no board', 'warn');
}

function populateExamples() {
  const sel = $('example-select');
  if (!sel || !S.state || !S.state.examples) return;
  if (sel.options.length > 1) return;
  for (const name of S.state.examples) {
    const opt = document.createElement('option');
    opt.value = name;
    opt.textContent = name;
    sel.appendChild(opt);
  }
}

async function loadExample(name) {
  if (!name) return;
  try {
    const st = await apiJson('/api/project/load', { name });
    S.board = null; S.sel = null; S.sch.fitted = false; S.bd.fitted = false;
    applyState(st);
    await loadBoard();
    notify('loaded ' + name);
  } catch (e) { notify(e.message, true); }
}

function renderNetlist() {
  const nl = S.state.netlist;
  if (!nl) return;
  let html = `<div class="kv"><span>Nets</span><span>${nl.nets.length}</span></div>`;
  html += `<div class="kv"><span>Pins</span><span>${nl.pins}</span></div>`;
  html += `<div class="kv"><span>Diagnostics</span><span>${nl.errors} err / ${nl.warnings} warn</span></div>`;
  for (const n of nl.nets.slice(0, 40)) {
    html += `<div class="netlist-net"><span>${esc(n.name)}</span><span>${n.nodes.length} pin${n.nodes.length === 1 ? '' : 's'}</span></div>`;
  }
  if (nl.nets.length > 40) html += `<div class="muted">… ${nl.nets.length - 40} more</div>`;
  for (const d of (nl.diagnostics || []).slice(0, 20)) {
    html += `<div class="diag ${esc(d.severity)}">${esc(d.severity)} ${esc(d.code)}: ${esc(d.message)}</div>`;
  }
  $('netlist-panel').innerHTML = html;
}

function renderSelection() {
  const el = $('selection');
  if (!S.sel) { el.innerHTML = '<span class="muted">Nothing selected</span>'; return; }
  if (S.sel.kind === 'component') {
    const c = (S.state.schematic.components || []).find((x) => x.id === S.sel.id);
    if (!c) { el.innerHTML = '<span class="muted">Missing</span>'; return; }
    el.innerHTML = `<div class="kv"><span>Refdes</span><span>${esc(c.refdes)}</span></div>
      <div class="kv"><span>Symbol</span><span>${esc(c.symbol)}</span></div>
      <div class="field"><label>Value</label><input id="sel-value" value="${esc(c.value)}"></div>
      <div class="kv"><span>Rotation</span><span>${c.rotation}°</span></div>`;
    const inp = $('sel-value');
    inp.addEventListener('keydown', (e) => { if (e.key === 'Enter') { inp.blur(); } });
    inp.addEventListener('change', async () => {
      try { const st = await apiJson('/api/schematic/value', { id: c.id, value: inp.value }); applyState(st); }
      catch (e) { notify(e.message, true); }
    });
  } else if (S.sel.kind === 'wire') {
    el.innerHTML = `<div class="kv"><span>Wire</span><span>#${S.sel.id}</span></div>`;
  } else {
    el.innerHTML = `<div class="kv"><span>Label</span><span>#${S.sel.id}</span></div>`;
  }
}

function renderDrcSummary() {
  const d = S.drc || (S.state && S.state.drc);
  const sum = $('drc-summary');
  const list = $('drc-list');
  if (!d) {
    sum.textContent = S.board ? 'No DRC run' : 'No board';
    list.innerHTML = '';
    setStatus('drc-status', 'DRC —', '');
    return;
  }
  sum.innerHTML = `<div class="kv"><span>Status</span><span>${d.clean ? 'clean' : d.errors + ' errors'}</span></div>
    <div class="kv"><span>Warnings</span><span>${d.warnings}</span></div>
    <div class="kv"><span>Checked items</span><span>${d.checked}</span></div>
    <div class="kv"><span>Unrouted</span><span>${d.unrouted}</span></div>
    <div class="kv"><span>Details</span><span>${(d.violations || []).length}${d.truncated ? '+' : ''}</span></div>`;
  let html = '';
  for (const v of (d.violations || []).slice(0, 60)) {
    html += `<div class="violation ${v.severity === 'warning' ? 'warning' : ''}"><b>${esc(v.kind)}</b> ${esc(v.layer || '')} · ${esc(v.message)}</div>`;
  }
  list.innerHTML = html;
  setStatus('drc-status', d.clean ? 'DRC clean' : `DRC ${d.errors}`, d.clean ? 'ok' : 'bad');
  if (S.board) S.board.drc = d;
}

function routeSnapshot(job) {
  if (!job) return;
  const el = $('route-progress');
  const pct = job.total > 0 ? Math.round(100 * job.routed / job.total) : (job.phase === 'done' ? 100 : 0);
  el.style.width = Math.max(0, Math.min(100, pct)) + '%';
  let text = 'Idle';
  if (job.running) text = `${job.phase} · ${job.net_name || 'starting'} · ${job.routed}/${job.total}`;
  else if (job.finished) text = job.error ? `failed: ${job.error}` : `finished · ${job.routed}/${job.total} nets`;
  $('route-log').textContent = text;
  const r = job.report;
  if (r) {
    setStatus('route-status', r.clean ? 'routed clean' : `route ${r.drc.errors} DRC`, r.clean ? 'ok' : 'bad');
  } else if (job.running) {
    setStatus('route-status', 'routing…', 'warn');
  }
}

function renderRouteStatus() {
  if (!S.state) return;
  if (S.state.job) routeSnapshot(S.state.job);
  if (S.state.route && (!S.state.job || !S.state.job.running)) {
    const r = S.state.route;
    setStatus('route-status', r.clean ? 'routed clean' : `route ${r.drc.errors} DRC`, r.clean ? 'ok' : 'bad');
  }
}

// ---------------------------------------------------------------- actions

function applyState(st) {
  S.state = st;
  if (!st.board || !st.board.present) {
    S.board = null;
    S.drc = null;
    S.bd.fitted = false;
  }
  renderState();
  renderSelection();
  drawSchematic();
  drawBoard();
  if (S.tab === 'export') refreshManifest();
}

async function loadState() {
  S.state = await apiJson('/api/state');
  renderState();
}

async function loadLibrary() {
  S.lib = await apiJson('/api/library');
  renderParts('');
}

async function loadBoard() {
  try {
    S.board = await apiJson('/api/board');
    try {
      S.drc = await apiJson('/api/drc');
    } catch (_) {
      S.drc = S.board.drc || S.state.drc;
    }
    S.board.drc = S.drc;
    S.bd.fitted = false;
    renderDrcSummary();
    drawBoard();
  } catch (e) {
    S.board = null;
    renderDrcSummary();
    drawBoard();
  }
}

function renderParts(filter) {
  if (!S.lib) return;
  const q = filter.toLowerCase();
  const parts = (S.lib.parts || []).filter((p) =>
    !q || p.name.toLowerCase().includes(q) || p.description.toLowerCase().includes(q) || p.category.includes(q));
  $('part-list').innerHTML = parts.map((p) =>
    `<button class="part ${S.pendingPart === p.name ? 'pending' : ''}" data-part="${esc(p.name)}" title="${esc(p.description)}"><span>${esc(p.name)}</span><small>${esc(p.category)}</small></button>`).join('');
  for (const b of $('part-list').querySelectorAll('.part')) {
    b.addEventListener('click', () => {
      S.pendingPart = b.dataset.part;
      S.tool = 'select';
      setTool('select');
      renderParts($('part-search').value);
      notify(`Click the canvas to place ${b.dataset.part}`, false);
    });
  }
}

function setTool(tool) {
  S.tool = tool;
  S.wireStart = null;
  $('tool-select').classList.toggle('active', tool === 'select');
  $('tool-wire').classList.toggle('active', tool === 'wire');
  $('tool-label').classList.toggle('active', tool === 'label');
  $('schematic-canvas').style.cursor = tool === 'select' ? 'default' : 'crosshair';
  drawSchematic();
}

async function placeAt(world) {
  if (!S.pendingPart) { notify('Pick a part in the library first', true); return; }
  const at = [Math.round(world[0] / GRID) * GRID, Math.round(world[1] / GRID) * GRID];
  try {
    const res = await apiJson('/api/schematic/place', { part: S.pendingPart, at });
    S.state = res.state;
    S.board = null;
    renderState();
    drawSchematic();
    notify(`placed ${S.pendingPart}`);
  } catch (e) { notify(e.message, true); }
}

async function moveComponent(c) {
  if (!c) return;
  const at = [Math.round(c.at[0] / GRID) * GRID, Math.round(c.at[1] / GRID) * GRID];
  try {
    const st = await apiJson('/api/schematic/move', { id: c.id, at });
    applyState(st);
  } catch (e) { notify(e.message, true); }
}

function scheduleDraw() {
  if (S.raf) return;
  S.raf = requestAnimationFrame(() => {
    S.raf = 0;
    if (S.tab === 'schematic') drawSchematic();
    else if (S.tab === 'board') drawBoard();
    else if (S.tab === 'simulate' && S.plot) drawPlot();
  });
}

// ---------------------------------------------------------------- events

function bindSchematic() {
  const canvas = $('schematic-canvas');
  canvas.addEventListener('mousedown', (e) => {
    if (!S.state) return;
    const rect = canvas.getBoundingClientRect();
    const sx = e.clientX - rect.left, sy = e.clientY - rect.top;
    const { w, h } = canvasSize(canvas);
    const world = schWorld(sx, sy, w, h);
    S.mouse.world = world;
    if (e.button !== 0) return;
    if (S.tool === 'wire') {
      const p = [Math.round(world[0] / GRID) * GRID, Math.round(world[1] / GRID) * GRID];
      if (!S.wireStart) { S.wireStart = p; drawSchematic(); return; }
      (async () => {
        try {
          const st = await apiJson('/api/schematic/wire', { points: [S.wireStart, p] });
          S.wireStart = null;
          applyState(st);
        } catch (err) { notify(err.message, true); }
      })();
      return;
    }
    if (S.tool === 'label') {
      const name = prompt('Net label name');
      if (!name) return;
      const p = [Math.round(world[0] / GRID) * GRID, Math.round(world[1] / GRID) * GRID];
      (async () => {
        try {
          const st = await apiJson('/api/schematic/label', {
            name, at: p, kind: (name === 'GND' || name === 'VCC') ? 'power' : 'local',
          });
          applyState(st);
        } catch (err) { notify(err.message, true); }
      })();
      return;
    }
    const hit = hitTestSchematic(sx, sy, w, h);
    S.sel = hit ? { kind: hit.kind, id: hit.id } : null;
    renderSelection();
    drawSchematic();
    if (hit && hit.kind === 'component') {
      S.drag = { comp: hit.comp, startX: sx, startY: sy, startAt: hit.comp.at.slice(), moved: false };
    } else {
      S.pan = { x: sx, y: sy, panX: S.sch.panX, panY: S.sch.panY };
    }
  });
  canvas.addEventListener('mousemove', (e) => {
    const rect = canvas.getBoundingClientRect();
    const sx = e.clientX - rect.left, sy = e.clientY - rect.top;
    const { w, h } = canvasSize(canvas);
    S.mouse.world = schWorld(sx, sy, w, h);
    if (S.drag) {
      const dx = sx - S.drag.startX, dy = sy - S.drag.startY;
      if (Math.hypot(dx, dy) > 2) S.drag.moved = true;
      const c = S.drag.comp;
      c.at = [S.drag.startAt[0] + dx / S.sch.scale, S.drag.startAt[1] - dy / S.sch.scale];
      if (S.drag.moved) recomputePins(c);
      scheduleDraw();
    } else if (S.pan) {
      S.sch.panX = S.pan.panX + (sx - S.pan.x);
      S.sch.panY = S.pan.panY + (sy - S.pan.y);
      scheduleDraw();
    } else if (S.tool === 'wire' && S.wireStart) {
      scheduleDraw();
    }
    $('schematic-hud').textContent = `${(S.mouse.world[0] / NM).toFixed(2)}, ${(S.mouse.world[1] / NM).toFixed(2)} mm`;
  });
  window.addEventListener('mouseup', (e) => {
    if (S.drag) {
      const d = S.drag;
      S.drag = null;
      if (d.moved) moveComponent(d.comp);
      else drawSchematic();
    }
    S.pan = null;
  });
  canvas.addEventListener('dblclick', (e) => {
    const rect = canvas.getBoundingClientRect();
    const { w, h } = canvasSize(canvas);
    const world = schWorld(e.clientX - rect.left, e.clientY - rect.top, w, h);
    if (S.pendingPart) placeAt(world);
  });
  canvas.addEventListener('wheel', (e) => {
    e.preventDefault();
    const factor = Math.exp(-e.deltaY * 0.0015);
    S.sch.scale = Math.max(2e-8, Math.min(0.002, S.sch.scale * factor));
    drawSchematic();
  }, { passive: false });
  canvas.addEventListener('mouseleave', () => { S.drag = null; S.pan = null; });
}

function bindBoard() {
  const canvas = $('board-canvas');
  let pan = null;
  canvas.addEventListener('mousedown', (e) => {
    const rect = canvas.getBoundingClientRect();
    pan = { x: e.clientX - rect.left, y: e.clientY - rect.top, panX: S.bd.panX, panY: S.bd.panY };
  });
  canvas.addEventListener('mousemove', (e) => {
    if (!pan) return;
    const rect = canvas.getBoundingClientRect();
    S.bd.panX = pan.panX + (e.clientX - rect.left - pan.x);
    S.bd.panY = pan.panY + (e.clientY - rect.top - pan.y);
    scheduleDraw();
  });
  window.addEventListener('mouseup', () => { pan = null; });
  canvas.addEventListener('wheel', (e) => {
    e.preventDefault();
    S.bd.scale = Math.max(1e-8, Math.min(0.01, S.bd.scale * Math.exp(-e.deltaY * 0.0015)));
    drawBoard();
  }, { passive: false });
  for (const id of ['show-top', 'show-bottom', 'show-rats']) {
    $(id).addEventListener('change', drawBoard);
  }
}

// ---------------------------------------------------------------- simulation plot

function drawPlot() {
  const canvas = $('plot-canvas');
  if (!canvas || S.tab !== 'simulate') return;
  const { ctx, w, h } = canvasSize(canvas);
  ctx.clearRect(0, 0, w, h);
  ctx.fillStyle = '#0a0f16'; ctx.fillRect(0, 0, w, h);
  const p = S.plot;
  if (!p) {
    ctx.fillStyle = '#8b949e'; ctx.font = '14px system-ui, sans-serif'; ctx.textAlign = 'center';
    ctx.fillText('Run a simulation to see waveforms', w / 2, h / 2);
    return;
  }
  const padL = 62, padR = 20, padT = 22, padB = 38;
  const iw = w - padL - padR, ih = h - padT - padB;
  if (p.mode === 'dc') {
    const nodes = p.nodes || [];
    let vmin = Math.min(0, ...nodes.map((n) => n.voltage));
    let vmax = Math.max(0, ...nodes.map((n) => n.voltage));
    if (vmax - vmin < 1e-9) { vmax = 1; vmin = -1; }
    drawGrid(ctx, padL, padT, iw, ih, vmin, vmax, null, null);
    const bw = iw / Math.max(1, nodes.length);
    nodes.forEach((n, i) => {
      const y0 = padT + ih * (vmax / (vmax - vmin));
      const y = padT + ih * (1 - (n.voltage - vmin) / (vmax - vmin));
      ctx.fillStyle = COLORS[i % COLORS.length];
      ctx.fillRect(padL + i * bw + bw * 0.18, Math.min(y0, y), Math.max(1, bw * 0.64), Math.abs(y0 - y));
      ctx.fillStyle = '#8b949e';
      ctx.font = '10px ui-monospace, monospace'; ctx.textAlign = 'center';
      ctx.fillText(n.name.slice(0, 12), padL + i * bw + bw / 2, padT + ih + 15);
    });
    return;
  }
  const times = p.time_ms || [];
  if (!times.length) return;
  let vmin = Infinity, vmax = -Infinity;
  for (const s of p.series || []) for (const v of s.values) { if (v < vmin) vmin = v; if (v > vmax) vmax = v; }
  if (!isFinite(vmin)) { vmin = 0; vmax = 1; }
  if (vmax - vmin < 1e-9) { vmax += 1; vmin -= 1; }
  const tmax = times[times.length - 1] || 1;
  drawGrid(ctx, padL, padT, iw, ih, vmin, vmax, tmax, 'time (ms)');
  (p.series || []).forEach((s, si) => {
    ctx.strokeStyle = s.color || COLORS[si % COLORS.length];
    ctx.lineWidth = 1.6;
    ctx.beginPath();
    s.values.forEach((v, i) => {
      const x = padL + (times[i] / tmax) * iw;
      const y = padT + ih * (1 - (v - vmin) / (vmax - vmin));
      if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    });
    ctx.stroke();
  });
}

function drawGrid(ctx, x, y, w, h, vmin, vmax, tmax, xlabel) {
  ctx.strokeStyle = '#1b2635'; ctx.lineWidth = 1;
  ctx.fillStyle = '#8b949e'; ctx.font = '10px ui-monospace, monospace';
  for (let i = 0; i <= 4; i++) {
    const yy = y + (h * i) / 4;
    ctx.beginPath(); ctx.moveTo(x, yy); ctx.lineTo(x + w, yy); ctx.stroke();
    const val = vmax - ((vmax - vmin) * i) / 4;
    ctx.textAlign = 'right';
    ctx.fillText(val.toFixed(2), x - 7, yy + 3);
  }
  for (let i = 0; i <= 4; i++) {
    const xx = x + (w * i) / 4;
    ctx.beginPath(); ctx.moveTo(xx, y); ctx.lineTo(xx, y + h); ctx.stroke();
    if (tmax) {
      ctx.textAlign = 'center';
      ctx.fillText(((tmax * i) / 4).toFixed(2), xx, y + h + 14);
    }
  }
  if (xlabel) {
    ctx.textAlign = 'center';
    ctx.fillText(xlabel, x + w / 2, y + h + 30);
  }
}

function renderLegend(p) {
  if (!p) { $('legend').innerHTML = ''; return; }
  if (p.mode === 'dc') {
    $('legend').innerHTML = (p.nodes || []).map((n, i) =>
      `<div class="item"><span class="swatch" style="background:${COLORS[i % COLORS.length]}"></span>${esc(n.name)} ${n.voltage.toFixed(4)} V</div>`).join('');
  } else {
    $('legend').innerHTML = (p.series || []).map((s, i) =>
      `<div class="item"><span class="swatch" style="background:${s.color || COLORS[i % COLORS.length]}"></span>${esc(s.name)}</div>`).join('');
  }
}

async function runSimulation() {
  if (!S.state) return;
  const btn = $('simulate-run');
  btn.disabled = true;
  $('sim-status').textContent = 'Running…';
  try {
    const mode = $('sim-mode').value;
    const body = { mode, method: $('sim-method').value };
    if (mode === 'tran') {
      body.tstep = Number($('sim-tstep').value) * 1e-6;
      body.tstop = Number($('sim-tstop').value) * 1e-3;
    }
    const res = await apiJson('/api/simulate', body);
    S.plot = res;
    drawPlot();
    renderLegend(res);
    const info = res.mode === 'dc'
      ? `DC operating point · ${res.iterations} Newton iterations · ${res.nodes.length} nodes`
      : `Transient ${res.method} · ${res.steps} steps · ${res.samples} plotted samples${res.truncated ? ' · truncated' : ''}`;
    $('sim-status').textContent = info;
    $('sim-headline').textContent = info;
    const errs = (res.diagnostics || []).filter((d) => d.severity === 'error').length;
    if (errs) notify(`${errs} netlist errors`, true);
  } catch (e) {
    $('sim-status').textContent = 'Failed: ' + e.message;
    $('sim-headline').textContent = 'Simulation failed';
    S.plot = null; drawPlot(); renderLegend(null);
    notify(e.message, true);
  } finally {
    btn.disabled = false;
  }
}

// ---------------------------------------------------------------- autoroute SSE

function startRouteSSE() {
  if (S.routeES) { S.routeES.close(); S.routeES = null; }
  const url = '/api/route/events?token=' + encodeURIComponent(token);
  const es = new EventSource(url);
  S.routeES = es;
  const onProgress = (ev) => {
    try {
      const job = JSON.parse(ev.data);
      routeSnapshot(job);
      if (job.events && job.events.length) {
        $('route-log').textContent = job.events.slice(-1)[0].phase + ' · ' + (job.events.slice(-1)[0].net_name || '');
      }
    } catch (_) {}
  };
  es.addEventListener('progress', onProgress);
  es.addEventListener('done', async (ev) => {
    onProgress(ev);
    es.close();
    S.routeES = null;
    try {
      S.state = await apiJson('/api/state');
      await loadBoard();
      renderState();
      drawBoard();
      const r = S.state.route;
      if (r) notify(r.clean ? 'Autoroute complete: clean' : `Autoroute finished with ${r.drc.errors} DRC errors`, !r.clean);
    } catch (e) { notify(e.message, true); }
  });
  es.onerror = () => {
    es.close();
    S.routeES = null;
    setStatus('route-status', 'route stream lost', 'warn');
  };
}

async function autoroute() {
  const btn = $('autoroute');
  btn.disabled = true;
  $('route-log').textContent = 'Starting…';
  try {
    await apiJson('/api/route/start', {});
    startRouteSSE();
  } catch (e) {
    notify(e.message, true);
    $('route-log').textContent = 'Failed: ' + e.message;
  } finally {
    btn.disabled = false;
  }
}

// ---------------------------------------------------------------- export

let previewUrl = null;

async function showPreview(layer) {
  try {
    const res = await api('/api/export/preview.png?layer=' + encodeURIComponent(layer));
    const blob = await res.blob();
    if (previewUrl) URL.revokeObjectURL(previewUrl);
    previewUrl = URL.createObjectURL(blob);
    const img = $('preview');
    img.src = previewUrl;
    img.style.display = 'block';
    $('preview-empty').style.display = 'none';
    $('export-status').textContent = `${layer} copper · ${Math.round(blob.size / 1024)} KiB PNG`;
  } catch (e) {
    $('preview-empty').style.display = 'block';
    $('preview-empty').textContent = e.message;
    notify(e.message, true);
  }
}

async function refreshManifest() {
  try {
    const m = await apiJson('/api/export/manifest');
    $('manifest').innerHTML = m.files.map((f) => `<div class="kv"><span>${esc(f.name)}</span><span>${f.bytes} B</span></div>`).join('');
    $('export-status').textContent = `${m.files.length} files ready`;
  } catch (e) {
    $('manifest').innerHTML = '<span class="muted">' + esc(e.message) + '</span>';
  }
}

async function downloadFile(kind) {
  const map = {
    'gerber-top': ['/api/export/gerber?layer=top', 'top.gbr'],
    'gerber-bottom': ['/api/export/gerber?layer=bottom', 'bottom.gbr'],
    'gerber-outline': ['/api/export/gerber?layer=outline', 'outline.gbr'],
    'gerber-silk': ['/api/export/gerber?layer=silk', 'silk.gbr'],
    'drill': ['/api/export/drill', 'board.drl'],
  };
  const [path, fallback] = map[kind];
  try {
    const res = await api(path);
    let name = fallback;
    const cd = res.headers.get('Content-Disposition') || '';
    const m = /filename="([^"]+)"/.exec(cd);
    if (m) name = m[1];
    const blob = await res.blob();
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = name;
    document.body.appendChild(a); a.click(); a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 2000);
    notify(`downloaded ${name}`);
  } catch (e) { notify(e.message, true); }
}

// ---------------------------------------------------------------- tabs and boot

function setTab(tab) {
  S.tab = tab;
  for (const t of document.querySelectorAll('.tab')) t.classList.toggle('active', t.dataset.tab === tab);
  for (const v of document.querySelectorAll('.view')) v.classList.toggle('active', v.id === 'view-' + tab);
  if (tab === 'board') {
    if (!S.board && S.state && S.state.board && S.state.board.present) loadBoard();
    else drawBoard();
  }
  if (tab === 'export') {
    if (!S.board && S.state && S.state.board && S.state.board.present) loadBoard();
    refreshManifest();
  }
  if (tab === 'simulate') drawPlot();
  if (tab === 'schematic') drawSchematic();
}

function bindUI() {
  for (const t of document.querySelectorAll('.tab')) t.addEventListener('click', () => setTab(t.dataset.tab));
  $('tool-select').addEventListener('click', () => setTool('select'));
  $('tool-wire').addEventListener('click', () => setTool('wire'));
  $('tool-label').addEventListener('click', () => setTool('label'));
  $('part-search').addEventListener('input', (e) => renderParts(e.target.value));
  $('load-demo').addEventListener('click', async () => {
    try {
      const st = await apiJson('/api/project/demo', {});
      S.board = null; S.sel = null; S.sch.fitted = false; S.bd.fitted = false;
      applyState(st);
      await loadBoard();
      notify('demo loaded');
    } catch (e) { notify(e.message, true); }
  });
  $('example-select').addEventListener('change', (e) => {
    loadExample(e.target.value).finally(() => { e.target.value = ''; });
  });
  $('new-project').addEventListener('click', async () => {
    try {
      const st = await apiJson('/api/project/reset', {});
      S.board = null; S.sel = null; S.sch.fitted = false; S.bd.fitted = false;
      applyState(st);
      notify('new project');
    } catch (e) { notify(e.message, true); }
  });
  $('rotate-sel').addEventListener('click', async () => {
    if (!S.sel || S.sel.kind !== 'component') return notify('Select a component', true);
    try { applyState(await apiJson('/api/schematic/rotate', { id: S.sel.id, delta: 90 })); }
    catch (e) { notify(e.message, true); }
  });
  $('mirror-sel').addEventListener('click', async () => {
    if (!S.sel || S.sel.kind !== 'component') return notify('Select a component', true);
    try { applyState(await apiJson('/api/schematic/mirror', { id: S.sel.id })); }
    catch (e) { notify(e.message, true); }
  });
  $('delete-sel').addEventListener('click', async () => {
    if (!S.sel) return notify('Nothing selected', true);
    try {
      const st = await apiJson('/api/schematic/delete', { kind: S.sel.kind, id: S.sel.id });
      S.sel = null; S.board = null;
      applyState(st);
    } catch (e) { notify(e.message, true); }
  });
  $('zoom-in').addEventListener('click', () => { S.sch.scale *= 1.3; drawSchematic(); });
  $('zoom-out').addEventListener('click', () => { S.sch.scale /= 1.3; drawSchematic(); });
  $('zoom-fit').addEventListener('click', () => { fitView('sch'); drawSchematic(); });
  $('simulate-run').addEventListener('click', runSimulation);
  $('generate-board').addEventListener('click', async () => {
    try {
      const st = await apiJson('/api/board/generate', {});
      S.board = null; S.bd.fitted = false;
      applyState(st);
      await loadBoard();
      notify('board generated');
    } catch (e) { notify(e.message, true); }
  });
  $('run-drc').addEventListener('click', async () => {
    try {
      const d = await apiJson('/api/drc');
      S.drc = d;
      renderDrcSummary();
      drawBoard();
      notify(d.clean ? 'DRC clean' : `${d.errors} DRC errors`, !d.clean);
    } catch (e) { notify(e.message, true); }
  });
  $('autoroute').addEventListener('click', autoroute);
  $('clear-routes').addEventListener('click', async () => {
    try {
      const st = await apiJson('/api/board/clear-routes', {});
      S.board = null;
      applyState(st);
      await loadBoard();
      notify('routes cleared');
    } catch (e) { notify(e.message, true); }
  });
  $('board-fit').addEventListener('click', () => { S.bd.fitted = false; fitView('bd'); drawBoard(); });
  $('preview-top').addEventListener('click', () => showPreview('top'));
  $('preview-bottom').addEventListener('click', () => showPreview('bottom'));
  for (const b of document.querySelectorAll('[data-export]')) b.addEventListener('click', () => downloadFile(b.dataset.export));
  window.addEventListener('resize', () => scheduleDraw());
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') { S.wireStart = null; S.pendingPart = null; renderParts($('part-search').value); setTool('select'); }
    if ((e.key === 'Delete' || e.key === 'Backspace') && S.sel && document.activeElement.tagName !== 'INPUT') {
      $('delete-sel').click();
    }
  });
}

async function boot() {
  if (!token) {
    notify('Missing Studio token. Launch Anvil from the terminal.', true);
    setStatus('net-status', 'no token', 'bad');
    return;
  }
  bindUI();
  bindSchematic();
  bindBoard();
  try {
    await Promise.all([loadState(), loadLibrary()]);
    S.sch.fitted = false;
    drawSchematic();
    setTab('schematic');
    if (S.state && S.state.board && S.state.board.present) {
      loadBoard().then(() => drawBoard()).catch(() => {});
    }
  } catch (e) {
    notify('Could not load project: ' + e.message, true);
  }
}

boot();
