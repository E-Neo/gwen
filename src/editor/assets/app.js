// @ts-check
/* gwen edit: command-based browser editor. TOML remains authoritative. */

const $ = (sel) => /** @type {HTMLElement} */ (document.querySelector(sel));
const $$ = (sel) => [...document.querySelectorAll(sel)];
const clamp = (value, min, max) => Math.min(max, Math.max(min, value));
const MOUSE_DRAG_THRESHOLD_PX = 6;
const TOUCH_DRAG_THRESHOLD_PX = 10;

const state = {
  project: null,
  slide: null,
  current: 0,
  selection: new Set(),
  zoom: 1,
  scale: 1,
  fitScale: 1,
  multi: false,
  drag: null,
  pinch: null,
  pointers: new Map(),
  textEdit: null,
  presenting: false,
  presentIndex: 0,
  lastTap: null,
  slideDrag: null,
  actionBusy: false,
  pollBusy: false,
  autoReloadBusy: false,
  autoReloadTimer: 0,
  reloadBlockedUntil: 0,
};

function setStatus(text, error = false) {
  const node = $('#status');
  node.textContent = text;
  node.classList.toggle('error', error);
}

let toastTimer = 0;
function toast(message, error = true) {
  const node = $('#toast');
  node.textContent = message;
  node.classList.toggle('error', error);
  node.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => { node.hidden = true; }, 7000);
}

async function api(path, options = {}) {
  const response = await fetch(path, {
    headers: { 'Content-Type': 'application/json' },
    ...options,
  });
  let body = null;
  try { body = await response.json(); } catch { body = null; }
  if (!response.ok) {
    const error = new Error(body && body.error ? body.error : response.statusText);
    (/** @type {any} */ (error)).status = response.status;
    throw error;
  }
  return body;
}

async function loadProject() {
  const project = await api('/api/project');
  state.project = project;
  state.current = Math.max(0, Math.min(state.current, project.slides.length - 1));
  $('#deck-title').textContent = project.title ? `· ${project.title}` : '';
  renderSlideList();
}

async function loadSlide(index, selection = null) {
  if (!state.project || !state.project.slides.length) return;
  closeTextEditor(false);
  const index_ = Math.max(0, Math.min(index, state.project.slides.length - 1));
  const result = await api(`/api/slide/${index_}`);
  state.current = index_;
  state.slide = result.slide;
  renderSlideSvg();
  if (selection) state.selection = new Set(selection);
  else state.selection = new Set([...state.selection].filter((n) => n < state.slide.shapes.length));
  updateActiveThumb();
  layoutStage();
  renderStage();
  renderInspector();
}

async function applyAction(action, options = {}) {
  closeTextEditor(false);
  if (!options.keepSheet) closeSheets();
  state.actionBusy = true;
  const previousFile = state.project && state.project.slides[state.current]
    ? state.project.slides[state.current].file
    : null;
  setStatus('Saving...');
  try {
    const result = await api('/api/action', { method: 'POST', body: JSON.stringify(action) });
    state.project = result.project;
    if (!options.keepSlide && typeof result.slide === 'number') {
      state.current = Math.max(0, Math.min(result.slide, result.project.slides.length - 1));
    } else if (previousFile) {
      const found = result.project.slides.findIndex((slide) => slide.file === previousFile);
      if (found >= 0) state.current = found;
    }
    const selection = Array.isArray(result.selection) && result.selection.length
      ? result.selection
      : options.keepSelection ? [...state.selection] : [];
    await loadSlide(state.current, selection);
    if (structuralAction(action.type)) renderSlideList();
    else refreshThumbnail(state.current);
    setStatus('Saved');
  } catch (error) {
    await handleError(error);
  } finally {
    state.actionBusy = false;
  }
}

async function historyAction(kind) {
  closeTextEditor(false);
  state.actionBusy = true;
  const previousFile = state.project && state.project.slides[state.current]
    ? state.project.slides[state.current].file
    : null;
  setStatus(kind === 'undo' ? 'Undoing...' : 'Redoing...');
  try {
    const result = await api(`/api/${kind}`, { method: 'POST' });
    state.project = result.project;
    let current = Math.min(state.current, result.project.slides.length - 1);
    if (previousFile) {
      const found = result.project.slides.findIndex((slide) => slide.file === previousFile);
      if (found >= 0) current = found;
    }
    await loadSlide(current);
    renderSlideList();
    setStatus('Saved');
  } catch (error) {
    await handleError(error);
  } finally {
    state.actionBusy = false;
  }
}

async function reloadFromDisk() {
  closeTextEditor(false);
  state.actionBusy = true;
  const previousFile = state.project && state.project.slides[state.current]
    ? state.project.slides[state.current].file
    : null;
  setStatus('Reloading...');
  try {
    const result = await api('/api/reload', { method: 'POST' });
    state.project = result.project ?? result;
    let current = Math.min(state.current, state.project.slides.length - 1);
    if (previousFile) {
      const found = state.project.slides.findIndex((slide) => slide.file === previousFile);
      if (found >= 0) current = found;
    }
    await loadSlide(current);
    renderSlideList();
    setStatus('Saved');
  } catch (error) {
    await handleError(error, false);
  } finally {
    state.actionBusy = false;
  }
}

function isUserEditing() {
  const active = document.activeElement;
  return !!active && (
    active.matches('input, textarea, select') || active.isContentEditable
  );
}

function startAutoReload() {
  if (state.autoReloadTimer) return;
  state.autoReloadTimer = window.setInterval(pollDiskStatus, 1500);
  document.addEventListener('visibilitychange', () => {
    if (!document.hidden) pollDiskStatus();
  });
}

async function pollDiskStatus() {
  if (!state.project || document.hidden) return;
  if (state.pollBusy || state.actionBusy || state.autoReloadBusy) return;
  if (state.drag || state.textEdit || state.presenting) return;
  if (isUserEditing()) return;
  if (Date.now() < state.reloadBlockedUntil) return;
  state.pollBusy = true;
  try {
    const status = await api('/api/status');
    if (status.changed) await autoReload();
  } catch {
    // A transient read failure is retried on the next tick.
  } finally {
    state.pollBusy = false;
  }
}

async function autoReload() {
  if (state.autoReloadBusy) return;
  state.autoReloadBusy = true;
  const previousFile = state.project && state.project.slides[state.current]
    ? state.project.slides[state.current].file
    : null;
  try {
    const result = await api('/api/reload', { method: 'POST' });
    state.project = result.project ?? result;
    let current = Math.min(state.current, state.project.slides.length - 1);
    if (previousFile) {
      const found = state.project.slides.findIndex((slide) => slide.file === previousFile);
      if (found >= 0) current = found;
    }
    state.selection.clear();
    await loadSlide(current, []);
    renderSlideList();
    setStatus('Saved');
    toast('Reloaded changes from disk', false);
  } catch (error) {
    // Keep the last valid editor state while an external write is partial or
    // invalid, then retry.
    state.reloadBlockedUntil = Date.now() + 3000;
    setStatus('Reload failed', true);
    const message = String(error && error.message ? error.message : error);
    toast(`Project changed on disk but could not be loaded: ${message}`);
  } finally {
    state.autoReloadBusy = false;
  }
}

async function handleError(error, tryReload = true) {
  const message = String(error && error.message ? error.message : error);
  setStatus('Error', true);
  toast(message);
  if (!tryReload) return;
  if (error && error.status === 409 && message.includes('changed on disk')) {
    await reloadFromDisk();
    return;
  }
  try {
    await loadProject();
    await loadSlide(Math.min(state.current, state.project.slides.length - 1));
  } catch (reloadError) {
    toast(String(reloadError && reloadError.message ? reloadError.message : reloadError));
  }
}

function shapeAt(index) {
  if (!state.slide) return null;
  return state.slide.shapes.find((shape) => shape.index === index) || null;
}

function selectedShapes() {
  return [...state.selection].map(shapeAt).filter(Boolean);
}

function structuralAction(type) {
  return [
    'add_slide',
    'delete_slide',
    'duplicate_slide',
    'move_slide',
    'add_section',
    'rename_section',
    'delete_section',
  ].includes(type);
}

function updateActiveThumb() {
  $$('.thumb').forEach((thumb) => {
    thumb.classList.toggle('active', Number(thumb.dataset.slide) === state.current);
  });
}

function refreshThumbnail(index) {
  const img = document.querySelector(`.thumb[data-slide="${index}"] img`);
  if (img) img.src = `/api/slide/${index}.svg?rev=${state.project.revision}`;
}

function renderSlideList() {
  const list = $('#slide-list');
  if (!state.project) return;
  list.replaceChildren();
  for (const [sectionIndex, section] of state.project.sections.entries()) {
    const block = document.createElement('div');
    block.className = 'section-block';
    block.dataset.section = String(sectionIndex);
    const head = document.createElement('div');
    head.className = 'section-head';
    const title = document.createElement('input');
    title.className = 'section-title-input';
    title.value = section.title || `Section ${sectionIndex + 1}`;
    title.dataset.old = title.value;
    title.setAttribute('aria-label', 'Section name');
    title.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') { event.preventDefault(); title.blur(); }
      else if (event.key === 'Escape') { title.value = title.dataset.old; title.blur(); }
    });
    title.addEventListener('change', () => {
      const value = title.value.trim();
      if (!value) {
        title.value = title.dataset.old;
        toast('Section names cannot be empty.');
        return;
      }
      if (value !== title.dataset.old) {
        applyAction({ type: 'rename_section', section: sectionIndex, title: value });
      }
    });
    const tools = document.createElement('div');
    tools.className = 'section-tools';
    const add = document.createElement('button');
    add.type = 'button';
    add.textContent = '+';
    add.title = 'Add a slide to this section';
    add.addEventListener('click', (event) => {
      event.stopPropagation();
      applyAction({ type: 'add_slide', section: sectionIndex });
    });
    const remove = document.createElement('button');
    remove.type = 'button';
    remove.textContent = '×';
    remove.title = 'Delete this section';
    remove.disabled = state.project.sections.length <= 1;
    remove.addEventListener('click', (event) => {
      event.stopPropagation();
      const name = section.title || `Section ${sectionIndex + 1}`;
      if (!window.confirm(`Delete section "${name}"? Its slides move to the previous section.`)) return;
      applyAction({ type: 'delete_section', section: sectionIndex });
    });
    tools.append(add, remove);
    head.append(title, tools);
    block.append(head);

    const body = document.createElement('div');
    body.className = 'section-body';
    body.dataset.sectionBody = String(sectionIndex);
    if (section.slides.length === 0) {
      const empty = document.createElement('div');
      empty.className = 'section-empty';
      empty.textContent = 'Drop a slide here';
      body.append(empty);
    } else {
      for (const slideIndex of section.slides) {
        const slide = state.project.slides[slideIndex];
        if (!slide) continue;
        const thumb = document.createElement('div');
        thumb.className = 'thumb' + (slideIndex === state.current ? ' active' : '');
        thumb.tabIndex = 0;
        thumb.setAttribute('role', 'button');
        thumb.dataset.slide = String(slideIndex);
        thumb.dataset.section = String(sectionIndex);
        const img = document.createElement('img');
        img.alt = slide.stem;
        img.src = `/api/slide/${slideIndex}.svg?rev=${state.project.revision}`;
        thumb.append(img);
        const meta = document.createElement('div');
        meta.className = 'thumb-meta';
        const name = document.createElement('span');
        name.className = 'thumb-name';
        name.textContent = slide.stem;
        const grip = document.createElement('span');
        grip.className = 'thumb-grip';
        grip.textContent = '⠿';
        grip.title = 'Drag to reorder';
        grip.setAttribute('aria-label', 'Drag to reorder');
        grip.addEventListener('pointerdown', (event) => {
          event.preventDefault();
          event.stopPropagation();
          startSlideDrag(event, slideIndex, thumb);
        });
        grip.addEventListener('click', (event) => event.stopPropagation());
        meta.append(name, grip);
        thumb.append(meta);
        thumb.addEventListener('click', () => {
          closeSheets();
          loadSlide(slideIndex);
        });
        thumb.addEventListener('keydown', (event) => {
          if (event.target !== thumb) return;
          if (event.altKey && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) {
            event.preventDefault();
            const to = event.key === 'ArrowUp' ? slideIndex - 1 : slideIndex + 1;
            if (to >= 0 && to < state.project.slides.length) {
              applyAction({ type: 'move_slide', from: slideIndex, to });
            }
            return;
          }
          if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault();
            closeSheets();
            loadSlide(slideIndex);
          }
        });
        body.append(thumb);
      }
    }
    block.append(body);
    list.append(block);
  }
  const addRow = document.createElement('div');
  addRow.className = 'section-add-row';
  const addSection = document.createElement('button');
  addSection.type = 'button';
  addSection.textContent = '+ Add section';
  addSection.addEventListener('click', () => command('add-section'));
  addRow.append(addSection);
  list.append(addRow);
}

function startSlideDrag(event, from, thumb) {
  const drag = {
    from,
    startY: event.clientY,
    active: false,
    target: null,
    after: false,
    section: null,
    thumb,
  };
  state.slideDrag = drag;
  const onMove = (moveEvent) => {
    if (moveEvent.pointerId !== event.pointerId || state.slideDrag !== drag) return;
    if (!drag.active && Math.abs(moveEvent.clientY - drag.startY) < 5) return;
    if (!drag.active) {
      drag.active = true;
      thumb.classList.add('dragging');
    }
    const under = document.elementFromPoint(moveEvent.clientX, moveEvent.clientY);
    clearSlideDrop();
    drag.target = null;
    drag.after = false;
    drag.section = null;

    const target = under && under.closest ? under.closest('.thumb') : null;
    if (target && target !== thumb) {
      const section = Number(target.dataset.section);
      const index = Number(target.dataset.slide);
      if (!Number.isFinite(section) || !Number.isFinite(index)) return;
      const rect = target.getBoundingClientRect();
      drag.target = index;
      drag.section = section;
      drag.after = moveEvent.clientY > rect.top + rect.height / 2;
      target.classList.add(drag.after ? 'drop-after' : 'drop-before');
      return;
    }

    const body = under && under.closest ? under.closest('[data-section-body]') : null;
    const block = under && under.closest ? under.closest('.section-block') : null;
    const dropBody = body || (block ? block.querySelector('[data-section-body]') : null);
    if (dropBody) {
      const section = Number(dropBody.dataset.sectionBody);
      if (!Number.isFinite(section)) return;
      drag.section = section;
      drag.after = true;
      dropBody.classList.add('drop-into');
    }
  };
  const cleanup = () => {
    document.removeEventListener('pointermove', onMove);
    document.removeEventListener('pointerup', onUp);
    document.removeEventListener('pointercancel', onCancel);
    state.slideDrag = null;
    thumb.classList.remove('dragging');
    clearSlideDrop();
  };
  const onCancel = (cancelEvent) => {
    if (cancelEvent.pointerId !== event.pointerId) return;
    cleanup();
  };
  const sectionOf = (slideIndex) => {
    const slide = state.project.slides[slideIndex];
    return slide ? slide.section : null;
  };
  const positionOf = (slideIndex) => {
    const section = sectionOf(slideIndex);
    if (section === null) return -1;
    return state.project.sections[section].slides.indexOf(slideIndex);
  };
  const onUp = (upEvent) => {
    if (upEvent.pointerId !== event.pointerId) return;
    const active = drag.active;
    const target = drag.target;
    const after = drag.after;
    const targetSection = drag.section;
    const fromIndex = drag.from;
    cleanup();
    if (!active || targetSection === null) return;
    const sourceSection = sectionOf(fromIndex);
    const sourcePosition = positionOf(fromIndex);
    let to;
    if (target !== null) {
      const targetPosition = state.project.sections[targetSection].slides.indexOf(target);
      if (targetPosition < 0) return;
      to = targetPosition;
      if (sourceSection === targetSection && sourcePosition < targetPosition) to -= 1;
      if (after) to += 1;
    } else {
      to = state.project.sections[targetSection].slides.length;
      if (sourceSection === targetSection) to -= 1;
    }
    to = Math.max(0, Math.min(to, state.project.sections[targetSection].slides.length));
    if (sourceSection === targetSection && to === sourcePosition) return;
    applyAction({
      type: 'move_slide',
      from: fromIndex,
      to,
      section: targetSection,
    });
  };
  document.addEventListener('pointermove', onMove);
  document.addEventListener('pointerup', onUp);
  document.addEventListener('pointercancel', onCancel);
}

function clearSlideDrop() {
  $$('.thumb.drop-before, .thumb.drop-after').forEach((node) => {
    node.classList.remove('drop-before', 'drop-after');
  });
  $$('.section-body.drop-into').forEach((node) => node.classList.remove('drop-into'));
}

function renderSlideSvg() {
  $('#slide-image').innerHTML = state.slide ? state.slide.svg : '';
}

function layoutStage() {
  if (!state.slide) return;
  const wrap = $('#stage-wrap');
  const padding = window.matchMedia('(max-width: 700px)').matches ? 16 : 32;
  const availableW = Math.max(120, wrap.clientWidth - padding);
  const availableH = Math.max(120, wrap.clientHeight - padding);
  const fit = Math.min(availableW / state.slide.width, availableH / state.slide.height);
  state.fitScale = fit;
  state.scale = fit * state.zoom;
  const stage = $('#stage');
  stage.style.width = `${state.slide.width * state.scale}px`;
  stage.style.height = `${state.slide.height * state.scale}px`;
  $('#zoom-label').textContent = `${Math.round(state.zoom * 100)}%`;
}

function boxStyle(box, shape) {
  box.style.left = `${shape.x * state.scale}px`;
  box.style.top = `${shape.y * state.scale}px`;
  box.style.width = `${shape.w * state.scale}px`;
  box.style.height = `${shape.h * state.scale}px`;
  box.style.translate = '';
  box.style.transform = shape.rotate ? `rotate(${shape.rotate}deg)` : '';
}

function renderStage() {
  const layer = $('#shape-layer');
  layer.replaceChildren();
  if (!state.slide) return;
  for (const shape of state.slide.shapes) {
    const box = document.createElement('div');
    box.className = 'shape-box';
    box.dataset.index = String(shape.index);
    box.title = `${shape.ty}${shape.placeholder ? ` (placeholder ${shape.placeholder})` : ''}`;
    if (shape.move_locked) box.classList.add('locked');
    if (state.selection.has(shape.index)) box.classList.add('selected');
    boxStyle(box, shape);
    layer.append(box);
  }
  if (state.selection.size === 1) addHandles();
  positionTextEditor();
  updateZoomBar();
}

function addHandles() {
  const index = [...state.selection][0];
  const shape = shapeAt(index);
  if (!shape) return;
  const box = /** @type {HTMLElement} */ ($(`.shape-box[data-index="${index}"]`));
  if (!box) return;
  if (!shape.resize_locked && !shape.rotate) {
    for (const dir of ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w']) {
      const handle = document.createElement('div');
      handle.className = `handle ${dir}`;
      handle.dataset.handle = dir;
      handle.dataset.index = String(index);
      box.append(handle);
    }
  }
  if (!shape.move_locked && shape.kind !== 'table') {
    const rotate = document.createElement('div');
    rotate.className = 'handle rotate';
    rotate.dataset.handle = 'rotate';
    rotate.dataset.index = String(index);
    rotate.title = 'Rotate';
    box.append(rotate);
  }
}

function updateZoomBar() {
  $('#zoom-label').textContent = `${Math.round(state.zoom * 100)}%`;
  $$('[data-cmd="multi"]').forEach((b) => {
    b.classList.toggle('active', state.multi);
    b.setAttribute('aria-pressed', String(state.multi));
  });
}

function selectShape(index, additive = false) {
  if (additive || state.multi) {
    if (state.selection.has(index)) state.selection.delete(index);
    else state.selection.add(index);
  } else {
    state.selection = new Set([index]);
  }
  renderStage();
  renderInspector();
}

function clearSelection() {
  state.selection.clear();
  renderStage();
  renderInspector();
}

function stagePoint(event) {
  const rect = $('#stage').getBoundingClientRect();
  return {
    x: (event.clientX - rect.left) / state.scale,
    y: (event.clientY - rect.top) / state.scale,
    px: event.clientX - rect.left,
    py: event.clientY - rect.top,
  };
}

function pointerDown(event) {
  if (event.button !== undefined && event.button !== 0) return;
  if (event.pointerType === 'touch') {
    state.pointers.set(event.pointerId, event);
    if (state.pointers.size >= 2) {
      // Capture every finger on the stage before cancelDrag() rebuilds the
      // shape boxes. Otherwise the second finger keeps an implicit capture on
      // a removed box and pinch/pan moves never reach the stage listener.
      captureStagePointers();
      if (!state.pinch) {
        cancelDrag();
        startPinch();
      }
      return;
    }
  }
  const target = /** @type {HTMLElement} */ (event.target);
  const handle = target.closest('.handle');
  const box = target.closest('.shape-box');
  if (handle) {
    event.preventDefault();
    event.stopPropagation();
    const index = Number(handle.dataset.index);
    if (handle.dataset.handle === 'rotate') startRotate(index, event);
    else startResize(index, handle.dataset.handle, event);
    return;
  }
  if (box) {
    event.preventDefault();
    const index = Number(box.dataset.index);
    const additive = event.shiftKey;
    const wasSelected = state.selection.has(index);

    // Defer touch double-tap editing until pointer-up so the second press can
    // also become a drag.
    let pendingText = null;
    if (event.pointerType === 'touch') {
      const now = performance.now();
      if (state.lastTap && state.lastTap.index === index && now - state.lastTap.time < 350) {
        state.lastTap = null;
        pendingText = index;
      } else {
        state.lastTap = { index, time: now };
      }
    }

    if (additive) {
      selectShape(index, true);
      return;
    }

    if (state.multi) {
      if (!wasSelected) {
        selectShape(index, true);
        return;
      }
      // Dragging a selected shape moves the whole selection; a tap without
      // movement removes it instead.
      beginMoveOrTap(index, event, null, index);
      return;
    }

    if (!wasSelected) {
      // Strict select-first: the first press only selects.
      selectShape(index, false);
      return;
    }

    const shape = shapeAt(index);
    if (shape && !shape.move_locked) {
      beginMoveOrTap(index, event, pendingText, null);
      return;
    }

    if (pendingText != null) {
      beginText(index);
      return;
    }
    renderStage();
    return;
  }
  if (event.target === $('#stage') || event.target === $('#shape-layer') || event.target === $('#slide-image')) {
    event.preventDefault();
    state.lastTap = null;
    clearSelection();
    startMarquee(event);
  }
}

function pointerMove(event) {
  if (event.pointerType === 'touch' && state.pointers.has(event.pointerId)) {
    state.pointers.set(event.pointerId, event);
    if (state.pinch) {
      movePinch();
      return;
    }
  }
  if (state.drag) moveDrag(event);
  else if (state.marquee) moveMarquee(event);
}

function pointerCancel(event) {
  if (event.pointerType === 'touch') state.pointers.delete(event.pointerId);
  if (state.pointers.size < 2) endPinch();
  cancelDrag();
}

function pointerUp(event) {
  if (event.pointerType === 'touch') {
    state.pointers.delete(event.pointerId);
    if (state.pinch && state.pointers.size < 2) endPinch();
  }
  if (state.drag) endDrag(event);
  else if (state.marquee) endMarquee(event);
}

function cancelDrag() {
  state.drag = null;
  state.marquee = null;
  $('#marquee').hidden = true;
  $('#guide-layer').replaceChildren();
  if (state.slide) renderStage();
}

/* ---------- moving ---------- */

function beginMoveOrTap(index, event, pendingText = null, pendingToggle = null) {
  startMove(index, event, { pendingText, pendingToggle });
  if (state.drag) return;
  if (pendingText != null) beginText(pendingText);
  else if (pendingToggle != null) selectShape(pendingToggle, true);
}

function startMove(index, event, options = {}) {
  const indices = state.multi || state.selection.has(index) ? [...state.selection] : [index];
  if (!indices.length) return;
  const shapes = indices.map(shapeAt).filter(Boolean);
  if (shapes.some((shape) => shape.move_locked)) {
    toast('Placeholder geometry comes from the master and cannot be moved.');
    return;
  }
  const start = stagePoint(event);
  const origins = new Map(shapes.map((shape) => [shape.index, { x: shape.x, y: shape.y }]));
  const bounds = selectionBounds(shapes);
  state.drag = {
    type: 'move',
    start,
    origins,
    bounds,
    moved: false,
    armed: false,
    pointerType: event.pointerType,
    pendingText: options.pendingText ?? null,
    pendingToggle: options.pendingToggle ?? null,
    event,
    targetsX: snapTargets('x'),
    targetsY: snapTargets('y'),
  };
  $('#stage').setPointerCapture(event.pointerId);
}

function selectionBounds(shapes) {
  const left = Math.min(...shapes.map((s) => s.x));
  const top = Math.min(...shapes.map((s) => s.y));
  const right = Math.max(...shapes.map((s) => s.x + s.w));
  const bottom = Math.max(...shapes.map((s) => s.y + s.h));
  return { x: left, y: top, w: right - left, h: bottom - top };
}

function moveDrag(event) {
  const drag = state.drag;
  if (!drag) return;
  const point = stagePoint(event);
  if (drag.type === 'move' && !drag.armed) {
    const threshold = drag.pointerType === 'touch'
      ? TOUCH_DRAG_THRESHOLD_PX
      : MOUSE_DRAG_THRESHOLD_PX;
    if (Math.hypot(point.px - drag.start.px, point.py - drag.start.py) < threshold) return;
    drag.armed = true;
    // Start from the threshold crossing so the shape does not jump. Wait for
    // the next move event before applying any delta (and any snap).
    drag.start = point;
    return;
  }
  let dx = point.x - drag.start.x;
  let dy = point.y - drag.start.y;
  let guides = [];
  if (drag.type === 'move' && !event.altKey) {
    const snapped = snapMove(drag.bounds, dx, dy, drag.targetsX, drag.targetsY);
    dx = snapped.dx;
    dy = snapped.dy;
    guides = snapped.guides;
  }
  if (drag.type === 'move') {
    drag.moved = drag.moved || Math.abs(dx) > 1e-6 || Math.abs(dy) > 1e-6;
    for (const [index] of drag.origins) {
      const box = $(`.shape-box[data-index="${index}"]`);
      if (box) {
        box.style.translate = `${dx * state.scale}px ${dy * state.scale}px`;
      }
    }
    drawGuides(guides);
    drag.dx = dx;
    drag.dy = dy;
    return;
  }
  if (drag.type === 'resize') {
    const { handle, original } = drag;
    let x = original.x, y = original.y, w = original.w, h = original.h;
    const right = original.x + original.w;
    const bottom = original.y + original.h;
    if (handle.includes('w')) { x = Math.min(original.x + dx, right - 0.1); w = right - x; }
    if (handle.includes('e')) { w = Math.max(0.1, original.w + dx); }
    if (handle.includes('n')) { y = Math.min(original.y + dy, bottom - 0.1); h = bottom - y; }
    if (handle.includes('s')) { h = Math.max(0.1, original.h + dy); }
    if (!event.altKey) {
      const snapped = snapResize({ x, y, w, h }, handle, drag.targetsX, drag.targetsY);
      x = snapped.x; y = snapped.y; w = snapped.w; h = snapped.h;
      guides = snapped.guides;
    }
    drag.rect = { x, y, w, h };
    const box = $(`.shape-box[data-index="${drag.index}"]`);
    if (box) {
      box.style.left = `${x * state.scale}px`;
      box.style.top = `${y * state.scale}px`;
      box.style.width = `${w * state.scale}px`;
      box.style.height = `${h * state.scale}px`;
    }
    drawGuides(guides);
    return;
  }
  if (drag.type === 'rotate') {
    const center = drag.center;
    const angle = Math.atan2(point.y - center.y, point.x - center.x) * 180 / Math.PI + 90;
    let rotate = angle + drag.angleOffset;
    if (event.shiftKey) rotate = Math.round(rotate / 15) * 15;
    drag.rotate = rotate;
    const box = $(`.shape-box[data-index="${drag.index}"]`);
    if (box) box.style.transform = `rotate(${rotate}deg)`;
  }
}

function endDrag(event) {
  const drag = state.drag;
  state.drag = null;
  $('#guide-layer').replaceChildren();
  if (!drag) return;
  if (drag.type === 'move') {
    if (drag.moved) {
      applyAction({ type: 'move', slide: state.current, indices: [...drag.origins.keys()], dx: drag.dx, dy: drag.dy }, { keepSelection: true });
    } else if (!drag.armed && drag.pendingText != null) {
      beginText(drag.pendingText);
    } else if (!drag.armed && drag.pendingToggle != null) {
      selectShape(drag.pendingToggle, true);
    } else {
      renderStage();
    }
    return;
  }
  if (drag.type === 'resize' && drag.rect) {
    const rect = drag.rect;
    applyAction({ type: 'resize', slide: state.current, index: drag.index, x: rect.x, y: rect.y, w: rect.w, h: rect.h }, { keepSelection: true });
  } else if (drag.type === 'rotate' && drag.rotate !== undefined) {
    applyAction({ type: 'set_shape', slide: state.current, index: drag.index, keys: { rotate: Math.round(drag.rotate * 10) / 10 } }, { keepSelection: true });
  }
}

/* ---------- resizing / rotation ---------- */

function startResize(index, handle, event) {
  const shape = shapeAt(index);
  if (!shape || shape.resize_locked || shape.rotate) return;
  state.drag = {
    type: 'resize',
    index,
    handle,
    start: stagePoint(event),
    original: { x: shape.x, y: shape.y, w: shape.w, h: shape.h },
    rect: null,
    targetsX: snapTargets('x'),
    targetsY: snapTargets('y'),
  };
  $('#stage').setPointerCapture(event.pointerId);
}

function startRotate(index, event) {
  const shape = shapeAt(index);
  if (!shape || shape.move_locked || shape.kind === 'table') return;
  const center = {
    x: shape.x + shape.w / 2,
    y: shape.y + shape.h / 2,
  };
  const start = stagePoint(event);
  const startAngle = Math.atan2(start.y - center.y, start.x - center.x) * 180 / Math.PI + 90;
  state.drag = {
    type: 'rotate',
    index,
    start,
    center,
    original: shape.rotate || 0,
    angleOffset: (shape.rotate || 0) - startAngle,
  };
  $('#stage').setPointerCapture(event.pointerId);
}

/* ---------- snapping ---------- */

function snapTargets(axis) {
  const values = [0, axis === 'x' ? state.slide.width : state.slide.height];
  values.push(values[1] / 2);
  for (const shape of state.slide.shapes) {
    if (state.selection.has(shape.index)) continue;
    if (axis === 'x') values.push(shape.x, shape.x + shape.w, shape.x + shape.w / 2);
    else values.push(shape.y, shape.y + shape.h, shape.y + shape.h / 2);
  }
  return values;
}

function nearest(value, targets, threshold) {
  let best = null;
  for (const target of targets) {
    const distance = Math.abs(target - value);
    if (distance <= threshold && (!best || distance < best.distance)) best = { target, distance };
  }
  return best;
}

function snapMove(bounds, dx, dy, xTargets, yTargets) {
  const threshold = 7 / state.scale;
  const guides = [];
  const xCandidates = [bounds.x + dx, bounds.x + bounds.w / 2 + dx, bounds.x + bounds.w + dx];
  const yCandidates = [bounds.y + dy, bounds.y + bounds.h / 2 + dy, bounds.y + bounds.h + dy];
  for (const value of xCandidates) {
    const hit = nearest(value, xTargets, threshold);
    if (hit) {
      dx += hit.target - value;
      guides.push({ axis: 'v', value: hit.target });
      break;
    }
  }
  for (const value of yCandidates) {
    const hit = nearest(value, yTargets, threshold);
    if (hit) {
      dy += hit.target - value;
      guides.push({ axis: 'h', value: hit.target });
      break;
    }
  }
  return { dx, dy, guides };
}

function snapResize(rect, handle, xTargets, yTargets) {
  const threshold = 7 / state.scale;
  const guides = [];
  if (handle.includes('w')) {
    const hit = nearest(rect.x, xTargets, threshold);
    if (hit) { const right = rect.x + rect.w; rect.x = hit.target; rect.w = right - rect.x; guides.push({ axis: 'v', value: hit.target }); }
  }
  if (handle.includes('e')) {
    const right = rect.x + rect.w;
    const hit = nearest(right, xTargets, threshold);
    if (hit) { rect.w = hit.target - rect.x; guides.push({ axis: 'v', value: hit.target }); }
  }
  if (handle.includes('n')) {
    const hit = nearest(rect.y, yTargets, threshold);
    if (hit) { const bottom = rect.y + rect.h; rect.y = hit.target; rect.h = bottom - rect.y; guides.push({ axis: 'h', value: hit.target }); }
  }
  if (handle.includes('s')) {
    const bottom = rect.y + rect.h;
    const hit = nearest(bottom, yTargets, threshold);
    if (hit) { rect.h = hit.target - rect.y; guides.push({ axis: 'h', value: hit.target }); }
  }
  return { ...rect, guides };
}

function drawGuides(guides) {
  const layer = $('#guide-layer');
  layer.replaceChildren();
  for (const guide of guides) {
    const line = document.createElement('div');
    line.className = `guide ${guide.axis}`;
    if (guide.axis === 'v') line.style.left = `${guide.value * state.scale}px`;
    else line.style.top = `${guide.value * state.scale}px`;
    layer.append(line);
  }
}

/* ---------- marquee / pan / pinch ---------- */

function startMarquee(event) {
  const start = stagePoint(event);
  state.marquee = { start, current: start };
  const node = $('#marquee');
  node.hidden = false;
  updateMarqueeNode();
  $('#stage').setPointerCapture(event.pointerId);
}

function moveMarquee(event) {
  if (!state.marquee) return;
  state.marquee.current = stagePoint(event);
  updateMarqueeNode();
}

function updateMarqueeNode() {
  if (!state.marquee) return;
  const { start, current } = state.marquee;
  const node = $('#marquee');
  node.style.left = `${Math.min(start.px, current.px)}px`;
  node.style.top = `${Math.min(start.py, current.py)}px`;
  node.style.width = `${Math.abs(current.px - start.px)}px`;
  node.style.height = `${Math.abs(current.py - start.py)}px`;
}

function endMarquee() {
  if (!state.marquee) return;
  const { start, current } = state.marquee;
  state.marquee = null;
  $('#marquee').hidden = true;
  const x1 = Math.min(start.x, current.x), x2 = Math.max(start.x, current.x);
  const y1 = Math.min(start.y, current.y), y2 = Math.max(start.y, current.y);
  if (x2 - x1 < 0.02 && y2 - y1 < 0.02) return;
  const hits = state.slide.shapes
    .filter((shape) => !(shape.x > x2 || shape.x + shape.w < x1 || shape.y > y2 || shape.y + shape.h < y1))
    .map((shape) => shape.index);
  state.selection = new Set(hits);
  renderStage();
  renderInspector();
}

function captureStagePointers() {
  const stage = $('#stage');
  for (const pointerId of state.pointers.keys()) {
    try {
      stage.setPointerCapture(pointerId);
    } catch {
      // The pointer may already be gone; the remaining pointers still work.
    }
  }
}

function startPinch() {
  const points = [...state.pointers.values()].slice(0, 2);
  if (points.length < 2) return;
  const distance = Math.hypot(points[0].clientX - points[1].clientX, points[0].clientY - points[1].clientY);
  const center = {
    x: (points[0].clientX + points[1].clientX) / 2,
    y: (points[0].clientY + points[1].clientY) / 2,
  };
  const stage = $('#stage').getBoundingClientRect();
  state.pinch = {
    startDistance: Math.max(1, distance),
    startZoom: state.zoom,
    // Slide coordinate under the two-finger center at pinch start. Keeping it
    // under the moving center makes pinch zoom and two-finger pan coexist.
    anchor: {
      x: (center.x - stage.left) / state.scale,
      y: (center.y - stage.top) / state.scale,
    },
  };
}

function movePinch() {
  if (!state.pinch) return;
  const points = [...state.pointers.values()].slice(0, 2);
  if (points.length < 2) return;
  const distance = Math.hypot(points[0].clientX - points[1].clientX, points[0].clientY - points[1].clientY);
  const zoom = clamp(state.pinch.startZoom * distance / state.pinch.startDistance, 0.25, 4);
  if (Math.abs(zoom - state.zoom) >= 0.005) {
    state.zoom = zoom;
    layoutStage();
    renderStage();
  }
  const center = {
    x: (points[0].clientX + points[1].clientX) / 2,
    y: (points[0].clientY + points[1].clientY) / 2,
  };
  const wrap = $('#stage-wrap');
  const stage = $('#stage').getBoundingClientRect();
  const targetLeft = center.x - state.pinch.anchor.x * state.scale;
  const targetTop = center.y - state.pinch.anchor.y * state.scale;
  wrap.scrollLeft += stage.left - targetLeft;
  wrap.scrollTop += stage.top - targetTop;
}

function endPinch() {
  state.pinch = null;
}

/* ---------- text editing ---------- */

function beginText(index) {
  const shape = shapeAt(index);
  if (!shape) return;
  if (shape.paragraphs.length) {
    toast('This shape uses paragraphs; edit them in Properties.');
    return;
  }
  if (shape.kind === 'image' || shape.kind === 'table') {
    toast('Only text and shape objects can hold text.');
    return;
  }
  const editor = $('#text-editor');
  const input = /** @type {HTMLTextAreaElement} */ ($('#text-input'));
  const current = shape.text ?? '';
  state.textEdit = { index, original: current, value: current };
  input.value = current;
  editor.hidden = false;
  positionTextEditor();
  input.focus();
  input.setSelectionRange(input.value.length, input.value.length);
  resizeTextEditor();
}

function positionTextEditor() {
  if (!state.textEdit) return;
  const shape = shapeAt(state.textEdit.index);
  if (!shape) return;
  const editor = $('#text-editor');
  const input = /** @type {HTMLTextAreaElement} */ ($('#text-input'));
  editor.style.left = `${shape.x * state.scale}px`;
  editor.style.top = `${shape.y * state.scale}px`;
  editor.style.width = `${Math.max(shape.w * state.scale, 160)}px`;
  editor.style.height = `${Math.max(shape.h * state.scale, 100)}px`;
  const points = Number(shape.effective.fontSize) || 18;
  input.style.fontSize = `${Math.max(8, points * state.scale / 72)}px`;
  input.style.fontFamily = shape.effective.fontFace
    ? `"${shape.effective.fontFace}", system-ui, sans-serif`
    : 'system-ui, sans-serif';
}
function resizeTextEditor() {
  if (!state.textEdit) return;
  const editor = $('#text-editor');
  const viewport = window.visualViewport;
  if (viewport) {
    const available = viewport.height - 120;
    editor.style.maxHeight = `${Math.max(120, available)}px`;
  }
  editor.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
}
function closeTextEditor(commit) {
  if (!state.textEdit) return;
  const edit = state.textEdit;
  const input = /** @type {HTMLTextAreaElement} */ ($('#text-input'));
  const value = input.value;
  state.textEdit = null;
  $('#text-editor').hidden = true;
  if (commit && value !== edit.original) {
    applyAction({ type: 'set_shape', slide: state.current, index: edit.index, keys: { text: value } }, { keepSelection: true });
  }
}

function commitTextEdit() {
  closeTextEditor(true);
}

/* ---------- inspector ---------- */

function h(tag, props = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (key === 'class') node.className = value;
    else if (key === 'text') node.textContent = value;
    else if (key.startsWith('on')) node.addEventListener(key.slice(2).toLowerCase(), value);
    else if (key === 'value') node.value = value;
    else if (value === false) { /* omit boolean false attributes */ }
    else if (value !== undefined && value !== null) node.setAttribute(key, value);
  }
  for (const child of children.flat()) {
    if (child === null || child === undefined) continue;
    node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
}

function field(label, input, wide = false) {
  return h('label', { class: `field${wide ? ' wide' : ''}` }, h('span', { text: label }), input);
}

function numberField(label, value, onChange, disabled = false, step = '0.01') {
  const input = h('input', { type: 'number', step, value: Number.isFinite(value) ? value : '', disabled });
  input.addEventListener('change', () => onChange(Number(input.value)));
  return field(label, input);
}

function formatInches(value) {
  const number = Number(value);
  if (!Number.isFinite(number)) return '';
  return `${Number(number.toFixed(4))}in`;
}

function coordField(label, shape, key, onChange, disabled = false) {
  const raw = shape[`${key}_raw`];
  const input = h('input', {
    type: 'text',
    value: raw ?? formatInches(shape[key]),
    disabled,
    placeholder: '1in, 2.5cm, 72pt, 50%',
    title: 'Length: EMU integer, or a value with in, cm, mm, pt or %',
    autocomplete: 'off',
    spellcheck: 'false',
  });
  input.addEventListener('change', () => {
    const text = input.value.trim();
    if (!text) { renderInspector(); return; }
    onChange(text);
  });
  return field(label, input);
}

function colorValue(value) {
  if (!value) return '';
  if (typeof value === 'string') return value;
  if (typeof value === 'object' && value.color) return String(value.color);
  return '';
}

function applyShapeKeys(keys) {
  const index = [...state.selection][0];
  if (index === undefined) return;
  applyAction({ type: 'set_shape', slide: state.current, index, keys }, { keepSelection: true });
}

function applyGeometry(key, value) {
  const index = [...state.selection][0];
  if (index === undefined) return;
  applyAction(
    { type: 'set_geometry', slide: state.current, index, key, value },
    { keepSelection: true },
  );
}

function renderInspector() {
  const body = $('#inspector-body');
  body.replaceChildren();
  const shapes = selectedShapes();
  if (!shapes.length) {
    if (!state.slide) return;
    const colorInput = h('input', { type: 'color', value: normalizeHex(state.slide.background) || '#ffffff' });
    colorInput.addEventListener('change', () => {
      applyAction({ type: 'set_slide', slide: state.current, background: colorInput.value }, { keepSelection: true, keepSheet: true });
    });
    const clearBackground = h('button', {
      text: 'Clear',
      title: 'Use the master background',
      onclick: () => applyAction({ type: 'set_slide', slide: state.current, background: null }, { keepSelection: true, keepSheet: true }),
    });
    const notes = h('textarea', { rows: 4, placeholder: 'Speaker notes', value: state.slide.notes || '' });
    notes.addEventListener('change', () => {
      applyAction({ type: 'set_slide', slide: state.current, notes: notes.value }, { keepSelection: true, keepSheet: true });
    });
    body.append(
      h('div', { class: 'inspector-section' },
        h('h3', { text: 'Slide' }),
        field('Name', h('input', { value: state.project.slides[state.current]?.stem || '', disabled: true })),
        h('p', { class: 'note', text: `${state.slide.width.toFixed(2)} × ${state.slide.height.toFixed(2)} in` }),
        h('p', { class: 'note', text: 'Select a shape to edit it. Drag a selected shape to move it; drag handles to resize; double-click a shape to edit its text.' }),
      ),
      h('div', { class: 'inspector-section' },
        h('h3', { text: 'Background' }),
        h('div', { class: 'color-row' }, colorInput, clearBackground),
        h('p', { class: 'note', text: 'Clear removes the slide override and shows the master background.' }),
      ),
      h('div', { class: 'inspector-section' },
        h('h3', { text: 'Speaker notes' }),
        notes,
      ),
      h('div', { class: 'inspector-section' },
        h('h3', { text: 'Slide actions' }),
        h('div', { class: 'inline wrap' },
          h('button', { text: 'Duplicate slide', onclick: () => applyAction({ type: 'duplicate_slide', slide: state.current }) }),
          h('button', { text: 'Delete slide', onclick: () => applyAction({ type: 'delete_slide', slide: state.current }) }),
        ),
      ),
    );
    return;
  }
  if (shapes.length > 1) {
    const indices = shapes.map((shape) => shape.index);
    const align = (edge) => applyAction({ type: 'align', slide: state.current, indices, edge }, { keepSelection: true, keepSheet: true });
    const distribute = (axis) => applyAction({ type: 'distribute', slide: state.current, indices, axis }, { keepSelection: true, keepSheet: true });
    body.append(
      h('div', { class: 'inspector-section' },
        h('h3', { text: `${shapes.length} shapes selected` }),
        h('p', { class: 'note', text: 'Move the selection together, or align and distribute it here.' }),
        h('div', { class: 'inline wrap' },
          h('button', { text: 'Duplicate', onclick: () => bulk('duplicate') }),
          h('button', { text: 'Delete', onclick: () => bulk('delete') }),
          h('button', { text: 'Bring front', onclick: () => bulk('front') }),
          h('button', { text: 'Send back', onclick: () => bulk('back') }),
        ),
      ),
      h('div', { class: 'inspector-section' },
        h('h3', { text: 'Align' }),
        h('div', { class: 'inline wrap' },
          h('button', { text: 'Left', onclick: () => align('left') }),
          h('button', { text: 'Center', onclick: () => align('center') }),
          h('button', { text: 'Right', onclick: () => align('right') }),
          h('button', { text: 'Top', onclick: () => align('top') }),
          h('button', { text: 'Middle', onclick: () => align('middle') }),
          h('button', { text: 'Bottom', onclick: () => align('bottom') }),
        ),
      ),
      h('div', { class: 'inspector-section' },
        h('h3', { text: 'Distribute' }),
        h('div', { class: 'inline wrap' },
          h('button', { text: 'Horizontally', disabled: shapes.length < 3, onclick: () => distribute('horizontal') }),
          h('button', { text: 'Vertically', disabled: shapes.length < 3, onclick: () => distribute('vertical') }),
        ),
        h('p', { class: 'note', text: 'Distribute needs at least three selected shapes.' }),
      ),
    );
    return;
  }
  const shape = shapes[0];
  const shapeIndex = shape.index;
  const effective = shape.effective || {};
  body.append(h('div', { class: 'inspector-section' },
    h('h3', { text: `${shape.ty} · #${shape.index}` }),
    shape.placeholder ? h('p', { class: 'note', text: `Placeholder “${shape.placeholder}”. Geometry comes from the master; text and styles are editable.` }) : null,
    shape.kind === 'table' ? h('p', { class: 'note', text: 'Tables can be moved in this phase; table resizing and cell editing are coming next.' }) : null,
    shape.image_src ? h('p', { class: 'note', text: `Image: ${shape.image_src}` }) : null,
    shape.image_src ? h('div', { class: 'inline' }, h('button', { text: 'Replace image', onclick: () => pickImage() })) : null,
  ));
  body.append(h('div', { class: 'inspector-section' },
    h('h3', { text: 'Geometry' }),
    h('div', { class: 'field-grid' },
      coordField('X', shape, 'x', (v) => applyGeometry('x', v), shape.move_locked),
      coordField('Y', shape, 'y', (v) => applyGeometry('y', v), shape.move_locked),
      coordField('W', shape, 'w', (v) => applyGeometry('w', v), shape.resize_locked),
      coordField('H', shape, 'h', (v) => applyGeometry('h', v), shape.resize_locked),
      numberField('Rotate (°)', shape.rotate || 0, (v) => applyShapeKeys({ rotate: v }), shape.move_locked || shape.kind === 'table'),
    ),
    h('p', { class: 'note', text: 'Lengths accept in, cm, mm, pt or %; a bare integer is EMU.' }),
  ));
  const textSection = h('div', { class: 'inspector-section' }, h('h3', { text: 'Text' }));
  if (shape.paragraphs.length) {
    for (const [index, para] of shape.paragraphs.entries()) {
      const textarea = h('textarea', { value: para.text, rows: 3 });
      const apply = h('button', { text: 'Apply', onclick: () => applyAction({ type: 'set_paragraph', slide: state.current, index: shapeIndex, paragraph: index, text: textarea.value }, { keepSelection: true }) });
      textSection.append(h('div', { class: 'para-row' }, textarea, apply));
    }
  } else if (shape.kind !== 'image' && shape.kind !== 'table') {
    const textarea = h('textarea', { value: shape.text || '', rows: 5, placeholder: 'Type text for this shape' });
    textarea.addEventListener('change', () => applyShapeKeys({ text: textarea.value || null }));
    textSection.append(textarea);
  } else {
    textSection.append(h('p', { class: 'note', text: 'No text on this shape.' }));
  }
  body.append(textSection);
  body.append(h('div', { class: 'inspector-section' },
    h('h3', { text: 'Text style' }),
    h('div', { class: 'field-grid' },
      field('Font', h('input', { value: effective.fontFace || '', placeholder: 'theme font', onchange: (event) => applyShapeKeys({ font_face: event.target.value || null }) })),
      numberField('Size (pt)', Number(effective.fontSize) || 18, (v) => applyShapeKeys({ font_size: v }), false, '1'),
      field('Color', h('input', { value: colorValue(effective.color), placeholder: 'RRGGBB or theme', onchange: (event) => applyShapeKeys({ color: event.target.value || null }) })),
      field('Align', selectField(['left', 'center', 'right', 'justify'], effective.align || 'left', (v) => applyShapeKeys({ align: v }))),
      field('V-align', selectField(['top', 'middle', 'bottom'], effective.valign || 'top', (v) => applyShapeKeys({ valign: v }))),
    ),
    h('div', { class: 'inline' },
      checkboxField('Bold', !!effective.bold, (v) => applyShapeKeys({ bold: v || null })),
      checkboxField('Italic', !!effective.italic, (v) => applyShapeKeys({ italic: v || null })),
      checkboxField('Underline', !!effective.underline, (v) => applyShapeKeys({ underline: v || null })),
    ),
  ));
  const fillColor = colorValue(effective.fill);
  const lineColor = colorValue(effective.line);
  body.append(h('div', { class: 'inspector-section' },
    h('h3', { text: 'Fill and line' }),
    h('div', { class: 'field-grid' },
      field('Fill', h('input', { value: fillColor, placeholder: 'none', onchange: (event) => applyShapeKeys({ fill: event.target.value ? { color: event.target.value } : null }) })),
      field('Line', h('input', { value: lineColor, placeholder: 'none', onchange: (event) => applyShapeKeys({ line: event.target.value ? { color: event.target.value, width: Number(effective.line?.width) || 1 } : null }) })),
      numberField('Line width', Number(effective.line?.width) || 1, (v) => applyShapeKeys({ line: { color: lineColor || '333333', width: v } }), false, '0.1'),
    ),
  ));
  body.append(h('p', { class: 'note', text: 'Values shown are effective. Editing one creates an inline override; styles still apply to everything else.' }));
  body.append(h('div', { class: 'inspector-section' },
    h('h3', { text: 'Actions' }),
    h('div', { class: 'inline' },
      h('button', { text: 'Duplicate', onclick: () => bulk('duplicate') }),
      h('button', { text: 'Delete', onclick: () => bulk('delete') }),
      h('button', { text: 'Bring front', onclick: () => bulk('front') }),
      h('button', { text: 'Send back', onclick: () => bulk('back') }),
    ),
  ));
}

function selectField(options, value, onChange) {
  const select = document.createElement('select');
  for (const option of options) {
    const node = document.createElement('option');
    node.value = option;
    node.textContent = option;
    if (option === value) node.selected = true;
    select.append(node);
  }
  select.addEventListener('change', () => onChange(select.value));
  return select;
}

function checkboxField(label, value, onChange) {
  const input = h('input', { type: 'checkbox' });
  input.checked = value;
  input.addEventListener('change', () => onChange(input.checked));
  return h('label', { class: 'inline' }, input, h('span', { text: label }));
}

/* ---------- shape library and images ---------- */

const SHAPE_LIBRARY = [
  { type: 'rect', label: 'Rectangle', w: 2, h: 1.35, icon: '<rect x="4" y="5" width="32" height="18" rx="2"/>' },
  { type: 'roundRect', label: 'Rounded', w: 2, h: 1.35, icon: '<rect x="4" y="5" width="32" height="18" rx="7"/>' },
  { type: 'ellipse', label: 'Ellipse', w: 2, h: 1.35, icon: '<ellipse cx="20" cy="14" rx="16" ry="9"/>' },
  { type: 'triangle', label: 'Triangle', w: 2, h: 1.35, icon: '<polygon points="20,4 36,24 4,24"/>' },
  { type: 'diamond', label: 'Diamond', w: 2, h: 1.35, icon: '<polygon points="20,3 36,14 20,25 4,14"/>' },
  { type: 'pentagon', label: 'Pentagon', w: 2, h: 1.35, icon: '<polygon points="20,3 36,12 30,25 10,25 4,12"/>' },
  { type: 'hexagon', label: 'Hexagon', w: 2, h: 1.35, icon: '<polygon points="11,4 29,4 37,14 29,24 11,24 3,14"/>' },
  { type: 'star5', label: 'Star', w: 2, h: 1.35, icon: '<polygon points="20,3 23.5,12 33,12 25.5,18 28,27 20,21 12,27 14.5,18 7,12 16.5,12"/>' },
  { type: 'rightArrow', label: 'Arrow', w: 2.2, h: 1.2, icon: '<polygon points="4,9 24,9 24,4 37,14 24,24 24,19 4,19"/>' },
  { type: 'downArrow', label: 'Down arrow', w: 1.6, h: 1.8, icon: '<polygon points="15,3 25,3 25,16 35,16 20,26 5,16 15,16"/>' },
  { type: 'line', label: 'Line', w: 2.5, h: 0.05, icon: '<line x1="5" y1="24" x2="35" y2="4"/>' },
  { type: 'cloud', label: 'Cloud', w: 2, h: 1.35, icon: '<path d="M11 22a6 6 0 0 1 .6-11.9A8 8 0 0 1 27 9a6 6 0 0 1 1 12.9z"/>' },
];

const shapeIcon = (path) => '<svg viewBox="0 0 40 28" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true">' + path + '</svg>';

let shapePickerBuilt = false;
function openShapePicker() {
  if (!state.slide) return;
  if (!shapePickerBuilt) {
    const grid = $('#shape-grid');
    for (const shape of SHAPE_LIBRARY) {
      const button = h('button', { class: 'shape-choice', type: 'button', title: `Insert ${shape.label}`, onclick: () => insertShape(shape) });
      button.innerHTML = shapeIcon(shape.icon);
      button.append(h('span', { text: shape.label }));
      grid.append(button);
    }
    shapePickerBuilt = true;
  }
  $('#shape-picker').hidden = false;
}

function closeShapePicker() {
  $('#shape-picker').hidden = true;
}

function insertShape(spec) {
  closeShapePicker();
  if (!state.slide) return;
  const w = Math.max(0.1, Math.min(spec.w, state.slide.width - 0.5));
  const h = Math.max(0, Math.min(spec.h, state.slide.height - 0.5));
  applyAction({
    type: 'add_shape',
    slide: state.current,
    shape_type: spec.type,
    x: Math.max(0.25, (state.slide.width - w) / 2),
    y: Math.max(0.25, (state.slide.height - h) / 2),
    w,
    h,
  });
}

function pickImage() {
  if (!state.slide) return;
  $('#image-input').click();
}

function readImage(file) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = String(reader.result || '');
      const comma = result.indexOf(',');
      if (comma < 0) reject(new Error('Could not read the image'));
      else resolve(result.slice(comma + 1));
    };
    reader.onerror = () => reject(reader.error || new Error('Could not read the image'));
    reader.readAsDataURL(file);
  });
}

async function handleImageFile(file) {
  if (!file) return;
  if (file.size > 20 * 1024 * 1024) {
    toast('Images must be 20 MB or smaller.');
    return;
  }
  let data;
  try {
    data = await readImage(file);
  } catch (error) {
    toast(String(error && error.message ? error.message : error));
    return;
  }
  const selected = selectedShapes();
  const target = selected.length === 1 && selected[0].kind === 'image' ? selected[0] : null;
  if (target) {
    await applyAction({ type: 'replace_image', slide: state.current, index: target.index, name: file.name, data }, { keepSelection: true });
  } else {
    await applyAction({ type: 'add_image', slide: state.current, name: file.name, data });
  }
}

function normalizeHex(value) {
  const raw = colorValue(value).trim();
  if (/^#[0-9a-f]{6}$/i.test(raw)) return raw;
  if (/^[0-9a-f]{6}$/i.test(raw)) return `#${raw}`;
  return null;
}

/* ---------- commands ---------- */

function bulk(command) {
  const indices = [...state.selection];
  if (!indices.length) { toast('Select a shape first.', false); return; }
  if (command === 'delete') applyAction({ type: 'delete_shapes', slide: state.current, indices });
  else if (command === 'duplicate') applyAction({ type: 'duplicate_shapes', slide: state.current, indices });
  else applyAction({ type: 'z_order', slide: state.current, indices, op: command });
}

async function command(name) {
  switch (name) {
    case 'undo': return historyAction('undo');
    case 'redo': return historyAction('redo');
    case 'reload': return reloadFromDisk();
    case 'present': return present();
    case 'add-text': return addText();
    case 'shape': return openShapePicker();
    case 'image': return pickImage();
    case 'duplicate': return bulk('duplicate');
    case 'delete': return bulk('delete');
    case 'front': return bulk('front');
    case 'back': return bulk('back');
    case 'add-slide': return applyAction({ type: 'add_slide', after: state.current });
    case 'add-section': {
      const slide = state.project.slides[state.current];
      return applyAction({
        type: 'add_section',
        after: slide ? slide.section : null,
        title: 'New section',
      });
    }
    case 'duplicate-slide': return applyAction({ type: 'duplicate_slide', slide: state.current });
    case 'delete-slide': return applyAction({ type: 'delete_slide', slide: state.current });
    case 'zoom-in': state.zoom = clamp(state.zoom * 1.15, 0.25, 4); layoutStage(); renderStage(); return;
    case 'zoom-out': state.zoom = clamp(state.zoom / 1.15, 0.25, 4); layoutStage(); renderStage(); return;
    case 'fit': state.zoom = 1; layoutStage(); renderStage(); return;
    case 'multi': state.multi = !state.multi; updateZoomBar(); return;
  }
}

function addText() {
  if (!state.slide) return;
  const width = Math.min(6, state.slide.width - 2);
  const height = 1;
  applyAction({
    type: 'add_text',
    slide: state.current,
    x: Math.max(0.5, (state.slide.width - width) / 2),
    y: Math.max(0.5, (state.slide.height - height) / 2),
    w: width,
    h: height,
    text: 'New text',
  });
}

/* ---------- presentation ---------- */

function present() {
  if (!state.project) return;
  state.presenting = true;
  state.presentIndex = state.current;
  $('#presentation').hidden = false;
  showPresent();
  const root = $('#presentation');
  if (root.requestFullscreen) root.requestFullscreen().catch(() => {});
}
function showPresent() {
  const slide = state.project.slides[state.presentIndex];
  if (!slide) return;
  $('#present-image').src = `/api/slide/${state.presentIndex}.svg?rev=${state.project.revision}`;
  $('#present-count').textContent = `${state.presentIndex + 1} / ${state.project.slides.length}`;
}
function nextPresent(delta) {
  state.presentIndex = clamp(state.presentIndex + delta, 0, state.project.slides.length - 1);
  showPresent();
}
function stopPresent() {
  state.presenting = false;
  $('#presentation').hidden = true;
  if (document.fullscreenElement && document.exitFullscreen) document.exitFullscreen().catch(() => {});
}

/* ---------- mobile sheets ---------- */

function closeSheets() {
  document.body.classList.remove('slides-open', 'inspector-open');
  $('#scrim').hidden = true;
}
function openSheet(kind) {
  const wasOpen = document.body.classList.contains(kind);
  document.body.classList.remove('slides-open', 'inspector-open');
  if (!wasOpen) document.body.classList.add(kind);
  const open = document.body.classList.contains('slides-open') || document.body.classList.contains('inspector-open');
  $('#scrim').hidden = !open;
}

/* ---------- wiring ---------- */

function wire() {
  $('#stage').addEventListener('pointerdown', pointerDown);
  $('#stage').addEventListener('pointermove', pointerMove);
  $('#stage').addEventListener('pointerup', pointerUp);
  $('#stage').addEventListener('pointercancel', pointerCancel);
  $('#stage').addEventListener('dblclick', (event) => {
    const box = /** @type {HTMLElement} */ (event.target).closest('.shape-box');
    if (box) beginText(Number(box.dataset.index));
  });
  $('#stage').addEventListener('contextmenu', (event) => {
    const box = /** @type {HTMLElement} */ (event.target).closest('.shape-box');
    if (box) {
      event.preventDefault();
      selectShape(Number(box.dataset.index), event.shiftKey);
    }
  });
  $('#text-done').addEventListener('click', commitTextEdit);
  $('#text-cancel').addEventListener('click', () => closeTextEditor(false));
  $('#text-input').addEventListener('keydown', (event) => {
    if (event.key === 'Escape') { event.preventDefault(); closeTextEditor(false); }
    else if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) { event.preventDefault(); commitTextEdit(); }
  });
  $('#text-input').addEventListener('blur', (event) => {
    if (!state.textEdit) return;
    const related = /** @type {HTMLElement} */ (event.relatedTarget);
    if (related && related.closest('#text-editor')) return;
    if (window.matchMedia('(pointer: fine)').matches) commitTextEdit();
  });
  $('#toggle-slides').addEventListener('click', () => openSheet('slides-open'));
  $('#toggle-inspector').addEventListener('click', () => openSheet('inspector-open'));
  $('#scrim').addEventListener('click', closeSheets);
  $('#shape-close').addEventListener('click', closeShapePicker);
  $('#shape-picker').addEventListener('click', (event) => {
    if (event.target === $('#shape-picker')) closeShapePicker();
  });
  $('#image-input').addEventListener('change', async (event) => {
    const input = /** @type {HTMLInputElement} */ (event.target);
    const file = input.files && input.files[0];
    input.value = '';
    await handleImageFile(file);
  });
  $$('[data-cmd]').forEach((button) => {
    button.addEventListener('click', () => command(button.dataset.cmd));
  });
  $('#presentation').addEventListener('click', (event) => {
    if (/** @type {HTMLElement} */ (event.target).closest('.present-hud')) return;
    nextPresent(event.shiftKey ? -1 : 1);
  });
  window.addEventListener('resize', () => { layoutStage(); renderStage(); });
  if (window.visualViewport) {
    window.visualViewport.addEventListener('resize', () => {
      layoutStage();
      resizeTextEditor();
    });
  }
  new ResizeObserver(() => { layoutStage(); renderStage(); }).observe($('#stage-wrap'));
  document.addEventListener('keydown', onKeyDown);
}

function onKeyDown(event) {
  const target = /** @type {HTMLElement} */ (event.target);
  const editing = target.matches('input, textarea, select') || target.isContentEditable;
  if (state.presenting) {
    if (event.key === 'ArrowRight' || event.key === ' ' || event.key === 'PageDown') { event.preventDefault(); nextPresent(1); }
    else if (event.key === 'ArrowLeft' || event.key === 'PageUp') { event.preventDefault(); nextPresent(-1); }
    else if (event.key === 'Home') { event.preventDefault(); state.presentIndex = 0; showPresent(); }
    else if (event.key === 'End') { event.preventDefault(); state.presentIndex = state.project.slides.length - 1; showPresent(); }
    else if (event.key === 'Escape') { event.preventDefault(); stopPresent(); }
    return;
  }
  if (!$('#shape-picker').hidden) {
    if (event.key === 'Escape') { event.preventDefault(); closeShapePicker(); }
    return;
  }
  if (editing) return;
  const mod = event.ctrlKey || event.metaKey;
  if (mod && event.key.toLowerCase() === 'z') { event.preventDefault(); historyAction('undo'); return; }
  if (mod && (event.key.toLowerCase() === 'y' || (event.shiftKey && event.key.toLowerCase() === 'z'))) { event.preventDefault(); historyAction('redo'); return; }
  if (mod && event.key.toLowerCase() === 'd') { event.preventDefault(); bulk('duplicate'); return; }
  if (mod && event.key.toLowerCase() === 's') { event.preventDefault(); setStatus('Auto-saved'); return; }
  if (event.key === 'F5') { event.preventDefault(); present(); return; }
  if (event.key === 'Escape') { closeTextEditor(false); clearSelection(); closeSheets(); return; }
  if (event.key.startsWith('Arrow') && state.selection.size) {
    event.preventDefault();
    const amount = event.shiftKey ? 0.5 : 0.1;
    const dx = event.key === 'ArrowLeft' ? -amount : event.key === 'ArrowRight' ? amount : 0;
    const dy = event.key === 'ArrowUp' ? -amount : event.key === 'ArrowDown' ? amount : 0;
    applyAction({ type: 'move', slide: state.current, indices: [...state.selection], dx, dy }, { keepSelection: true });
    return;
  }
  if (event.key === 'Delete' || event.key === 'Backspace') {
    if (state.selection.size) { event.preventDefault(); bulk('delete'); }
    return;
  }
  if (event.key === 'Enter' || event.key === 'F2') {
    if (state.selection.size === 1) { event.preventDefault(); beginText([...state.selection][0]); }
  }
}

async function boot() {
  wire();
  try {
    await loadProject();
    if (!state.project.slides.length) {
      toast('This deck has no slides.', false);
      return;
    }
    await loadSlide(0);
    startAutoReload();
    setStatus('Saved');
  } catch (error) {
    setStatus('Error', true);
    toast(String(error && error.message ? error.message : error));
  }
}

boot();
