// WebDriver harness — injected on demand by src-tauri/src/webdriver.rs into
// whichever window a session is pointed at. Idempotent: every command
// evaluation prepends this file, so it must be cheap to re-run and must not
// reset the element registry (element references have to stay valid across
// commands, and only go stale when the document does).
(function () {
  if (window.__APPLY_WD__) return;

  // The W3C web element identifier. A JSON object carrying this key *is* an
  // element reference on the wire, in both directions.
  var KEY = 'element-6066-11e4-a52e-4f735466cecc';

  var elements = new Map(); // id -> Element
  var byElement = new WeakMap(); // Element -> id
  var seq = 0;

  function fail(code, message) {
    var error = new Error(message);
    error.__wdError = code;
    return error;
  }

  function ref(element) {
    var id = byElement.get(element);
    if (!id) {
      id = 'e' + ++seq;
      byElement.set(element, id);
      elements.set(id, element);
    }
    var wrapper = {};
    wrapper[KEY] = id;
    return wrapper;
  }

  // Elements detached from the document are stale, matching the spec's
  // "node is not in the document" rule. Ids from a previous document are
  // simply absent from the registry, which is the same failure.
  function deref(id) {
    var element = elements.get(id);
    if (!element) throw fail('stale element reference', 'element ' + id + ' is not known to this document');
    if (!element.isConnected) throw fail('stale element reference', 'element ' + id + ' is no longer attached to the DOM');
    return element;
  }

  function linkCandidates(root) {
    return Array.prototype.slice.call((root.querySelectorAll ? root : document).querySelectorAll('a'));
  }

  function locate(using, value, root, all) {
    root = root || document;
    switch (using) {
      case 'css selector':
        try {
          return all
            ? Array.prototype.slice.call(root.querySelectorAll(value))
            : [root.querySelector(value)].filter(Boolean);
        } catch (err) {
          throw fail('invalid selector', String(err && err.message ? err.message : err));
        }
      case 'tag name': {
        var tags = Array.prototype.slice.call(root.getElementsByTagName(value));
        return all ? tags : tags.slice(0, 1);
      }
      case 'link text': {
        var exact = linkCandidates(root).filter(function (a) {
          return (a.textContent || '').trim() === value;
        });
        return all ? exact : exact.slice(0, 1);
      }
      case 'partial link text': {
        var partial = linkCandidates(root).filter(function (a) {
          return (a.textContent || '').trim().indexOf(value) !== -1;
        });
        return all ? partial : partial.slice(0, 1);
      }
      case 'xpath': {
        var found = [];
        var result;
        try {
          result = document.evaluate(value, root, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
        } catch (err) {
          throw fail('invalid selector', String(err && err.message ? err.message : err));
        }
        for (var i = 0; i < result.snapshotLength; i++) {
          found.push(result.snapshotItem(i));
          if (!all) break;
        }
        return found;
      }
      default:
        throw fail('invalid argument', 'unsupported locator strategy: ' + using);
    }
  }

  function displayed(element) {
    if (!element.isConnected) return false;
    if (element.hidden) return false;
    var style = window.getComputedStyle(element);
    if (style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse') return false;
    if (parseFloat(style.opacity) === 0) return false;
    var rects = element.getClientRects();
    if (!rects.length) {
      // A zero-box element still counts when it lays out children, which is
      // how flex/grid wrappers and inline containers commonly measure.
      return element.getBoundingClientRect().height > 0 || element.getBoundingClientRect().width > 0;
    }
    return true;
  }

  function enabled(element) {
    if (typeof element.disabled === 'boolean') return !element.disabled;
    return !element.closest('[disabled]');
  }

  function rect(element) {
    var box = element.getBoundingClientRect();
    return { x: box.left + window.scrollX, y: box.top + window.scrollY, width: box.width, height: box.height };
  }

  function requireInteractable(element) {
    if (!displayed(element)) throw fail('element not interactable', 'element is not displayed');
    if (!enabled(element)) throw fail('element not interactable', 'element is disabled');
  }

  function mouseEvent(type, element) {
    var box = element.getBoundingClientRect();
    return new MouseEvent(type, {
      bubbles: true,
      cancelable: true,
      composed: true,
      view: window,
      clientX: box.left + box.width / 2,
      clientY: box.top + box.height / 2,
      button: 0,
    });
  }

  // Full pointer sequence before the native click, so listeners bound to
  // mousedown/mouseup (common in drag-aware UI code) see a real interaction.
  function click(element) {
    requireInteractable(element);
    if (element.scrollIntoView) element.scrollIntoView({ block: 'center', inline: 'center' });
    element.dispatchEvent(mouseEvent('mouseover', element));
    element.dispatchEvent(mouseEvent('mousemove', element));
    element.dispatchEvent(mouseEvent('mousedown', element));
    if (element.focus) {
      try { element.focus(); } catch (err) { /* focus is best-effort */ }
    }
    element.dispatchEvent(mouseEvent('mouseup', element));
    element.click();
  }

  // Frameworks that own the input's value (React, Vue with .lazy, …) only
  // notice a write that goes through the prototype's setter, not through the
  // instance property the framework has shadowed.
  function setValue(element, value) {
    var proto = element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    var descriptor = Object.getOwnPropertyDescriptor(proto, 'value');
    if (descriptor && descriptor.set) descriptor.set.call(element, value);
    else element.value = value;
  }

  function editable(element) {
    return element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement;
  }

  function key(name, code, element) {
    element.dispatchEvent(new KeyboardEvent('keydown', { key: name, code: code, bubbles: true, cancelable: true }));
    element.dispatchEvent(new KeyboardEvent('keyup', { key: name, code: code, bubbles: true, cancelable: true }));
  }

  // WebDriver encodes special keys as private-use code points; the ones below
  // are what a text field realistically needs. Anything else is typed as text.
  var SPECIAL = { '': 'Backspace', '': 'Tab', '': 'Enter', '': 'Enter', '': 'Escape' };

  function sendKeys(element, text) {
    requireInteractable(element);
    if (element.focus) element.focus();

    if (!editable(element)) {
      if (element.isContentEditable) {
        element.textContent = (element.textContent || '') + text;
        element.dispatchEvent(new InputEvent('input', { bubbles: true }));
        return;
      }
      if (element instanceof HTMLSelectElement) {
        var option = Array.prototype.slice.call(element.options).find(function (o) {
          return o.value === text || (o.textContent || '').trim() === text;
        });
        if (!option) throw fail('element not interactable', 'no option matching "' + text + '"');
        element.value = option.value;
        element.dispatchEvent(new Event('input', { bubbles: true }));
        element.dispatchEvent(new Event('change', { bubbles: true }));
        return;
      }
      throw fail('element not interactable', 'element does not accept text input');
    }

    for (var i = 0; i < text.length; i++) {
      var character = text[i];
      var special = SPECIAL[character];
      if (special === 'Backspace') {
        setValue(element, element.value.slice(0, -1));
        element.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'deleteContentBackward' }));
        key('Backspace', 'Backspace', element);
        continue;
      }
      if (special) {
        key(special, special, element);
        if (special === 'Enter' && element.form && element.form.requestSubmit) {
          element.dispatchEvent(new Event('change', { bubbles: true }));
        }
        continue;
      }
      element.dispatchEvent(new KeyboardEvent('keydown', { key: character, bubbles: true, cancelable: true }));
      setValue(element, element.value + character);
      element.dispatchEvent(new InputEvent('input', { bubbles: true, data: character, inputType: 'insertText' }));
      element.dispatchEvent(new KeyboardEvent('keyup', { key: character, bubbles: true, cancelable: true }));
    }
    element.dispatchEvent(new Event('change', { bubbles: true }));
  }

  function clear(element) {
    requireInteractable(element);
    if (editable(element)) {
      if (element.focus) element.focus();
      setValue(element, '');
      element.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'deleteContentBackward' }));
      element.dispatchEvent(new Event('change', { bubbles: true }));
      return;
    }
    if (element.isContentEditable) {
      element.textContent = '';
      element.dispatchEvent(new InputEvent('input', { bubbles: true }));
      return;
    }
    throw fail('element not interactable', 'element does not hold a clearable value');
  }

  // The JSON clone algorithm, trimmed to what this shell can actually carry
  // back over the invoke bridge. Cycles are an error, matching the spec.
  function serialize(value, seen) {
    seen = seen || [];
    if (value === null || value === undefined) return null;
    var type = typeof value;
    if (type === 'boolean' || type === 'string') return value;
    if (type === 'number') return Number.isFinite(value) ? value : null;
    if (type === 'function' || type === 'symbol') return null;
    if (value instanceof Element) return ref(value);
    if (value === window) throw fail('javascript error', 'cannot serialize the window object');
    if (seen.indexOf(value) !== -1) throw fail('javascript error', 'cannot serialize a cyclic structure');
    seen = seen.concat([value]);
    if (Array.isArray(value) || value instanceof NodeList || value instanceof HTMLCollection) {
      return Array.prototype.slice.call(value).map(function (item) { return serialize(item, seen); });
    }
    if (typeof value.toJSON === 'function') return serialize(value.toJSON(), seen);
    var out = {};
    for (var name in value) {
      if (Object.prototype.hasOwnProperty.call(value, name)) out[name] = serialize(value[name], seen);
    }
    return out;
  }

  function deserialize(value) {
    if (value === null || typeof value !== 'object') return value;
    if (Array.isArray(value)) return value.map(deserialize);
    if (Object.prototype.hasOwnProperty.call(value, KEY)) return deref(value[KEY]);
    var out = {};
    for (var name in value) {
      if (Object.prototype.hasOwnProperty.call(value, name)) out[name] = deserialize(value[name]);
    }
    return out;
  }

  // Every command body runs through here, so a thrown W3C error code reaches
  // Rust as data instead of being flattened into a message string.
  async function run(body) {
    try {
      return { status: 'ok', value: serialize(await body()) };
    } catch (err) {
      return {
        status: 'error',
        error: (err && err.__wdError) || 'javascript error',
        message: String(err && err.message ? err.message : err),
        stacktrace: String((err && err.stack) || ''),
      };
    }
  }

  window.__APPLY_WD__ = {
    KEY: KEY,
    fail: fail,
    ref: ref,
    deref: deref,
    locate: locate,
    displayed: displayed,
    enabled: enabled,
    rect: rect,
    click: click,
    sendKeys: sendKeys,
    clear: clear,
    serialize: serialize,
    deserialize: deserialize,
    run: run,
  };
})();
