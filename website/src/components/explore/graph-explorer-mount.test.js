/**
 * @vitest-environment jsdom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mount } from './graph-explorer.js';

const minimalData = {
  documents: [
    { id: 'a/one', doc_type: 'requirements', status: null, title: 'One', components: [] },
    { id: 'a/two', doc_type: 'design', status: null, title: 'Two', components: [] },
  ],
  edges: [{ from: 'a/one', to: 'a/two', kind: 'traces' }],
};

function createContainer() {
  const el = document.createElement('div');
  el.style.width = '800px';
  el.style.height = '600px';
  document.body.appendChild(el);
  // jsdom doesn't compute layout, so stub getBoundingClientRect
  el.getBoundingClientRect = () => ({
    x: 0,
    y: 0,
    width: 800,
    height: 600,
    top: 0,
    right: 800,
    bottom: 600,
    left: 0,
    toJSON() {},
  });
  return el;
}

describe('mount / unmount lifecycle', () => {
  /** @type {HTMLDivElement} */
  let container;

  beforeEach(() => {
    container = createContainer();
    // Reset hash
    window.location.hash = '';
    // Stub localStorage (jsdom may not provide a working one)
    const store = {};
    globalThis.localStorage = {
      getItem: (key) => store[key] ?? null,
      setItem: (key, val) => { store[key] = String(val); },
      removeItem: (key) => { delete store[key]; },
      clear: () => { for (const k in store) delete store[k]; },
      get length() { return Object.keys(store).length; },
      key: (i) => Object.keys(store)[i] ?? null,
    };
  });

  afterEach(() => {
    document.body.innerHTML = '';
    vi.restoreAllMocks();
  });

  it('mount returns an object with an unmount function', () => {
    const handle = mount(container, minimalData);
    expect(handle).toBeDefined();
    expect(typeof handle.unmount).toBe('function');
  });

  it('after unmount, document click listeners are removed', () => {
    const removeSpy = vi.spyOn(document, 'removeEventListener');
    const handle = mount(container, minimalData);

    handle.unmount();

    const removedTypes = removeSpy.mock.calls.map((c) => c[0]);
    expect(removedTypes).toContain('click');
  });

  it('after unmount, document mousemove listener is removed', () => {
    const removeSpy = vi.spyOn(document, 'removeEventListener');
    const handle = mount(container, minimalData);

    handle.unmount();

    const removedTypes = removeSpy.mock.calls.map((c) => c[0]);
    expect(removedTypes).toContain('mousemove');
  });

  it('after unmount, document mouseup listener is removed', () => {
    const removeSpy = vi.spyOn(document, 'removeEventListener');
    const handle = mount(container, minimalData);

    handle.unmount();

    const removedTypes = removeSpy.mock.calls.map((c) => c[0]);
    expect(removedTypes).toContain('mouseup');
  });

  it('after unmount, document keydown listener is removed', () => {
    const removeSpy = vi.spyOn(document, 'removeEventListener');
    const handle = mount(container, minimalData);

    handle.unmount();

    const removedTypes = removeSpy.mock.calls.map((c) => c[0]);
    expect(removedTypes).toContain('keydown');
  });

  it('after unmount, hashchange listener is unsubscribed', () => {
    const removeSpy = vi.spyOn(window, 'removeEventListener');
    const handle = mount(container, minimalData);

    handle.unmount();

    const removedTypes = removeSpy.mock.calls.map((c) => c[0]);
    expect(removedTypes).toContain('hashchange');
  });

  it('mount accepts a custom linkResolver as 5th parameter', () => {
    const customResolver = {
      evidenceLink: (file, line) => `vscode://file/${file}#${line}`,
      documentLink: (docId) => `#/doc/${encodeURIComponent(docId)}`,
      criterionLink: (docId, _criterionId) => `#/doc/${encodeURIComponent(docId)}`,
    };

    const handle = mount(container, minimalData, null, null, customResolver);
    expect(handle).toBeDefined();
    expect(typeof handle.unmount).toBe('function');
    handle.unmount();
  });

  it('mount without linkResolver still works (backward compatible)', () => {
    const handle = mount(container, minimalData, null, null);
    expect(handle).toBeDefined();
    expect(typeof handle.unmount).toBe('function');
    handle.unmount();
  });

  it('calling unmount multiple times is safe', () => {
    const handle = mount(container, minimalData);
    handle.unmount();
    expect(() => handle.unmount()).not.toThrow();
  });

  it('mount then unmount then mount does not duplicate document-level handlers', () => {
    const addSpy = vi.spyOn(document, 'addEventListener');

    const handle1 = mount(container, minimalData);
    const firstMountCalls = addSpy.mock.calls.filter(
      (c) => c[0] === 'click' || c[0] === 'mousemove' || c[0] === 'mouseup' || c[0] === 'keydown',
    ).length;

    handle1.unmount();
    document.body.innerHTML = '';
    const container2 = createContainer();
    addSpy.mockClear();

    const handle2 = mount(container2, minimalData);
    const secondMountCalls = addSpy.mock.calls.filter(
      (c) => c[0] === 'click' || c[0] === 'mousemove' || c[0] === 'mouseup' || c[0] === 'keydown',
    ).length;

    expect(secondMountCalls).toBe(firstMountCalls);
    handle2.unmount();
  });
});
