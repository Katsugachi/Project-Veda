// ---------------------------------------------------------------------------
// Keyed DOM morphing.
//
// The previous renderer replaced `app.innerHTML` on every state change. That
// destroyed and rebuilt every node, which caused three separate defects:
//
//   * CSS transitions never ran, because the "before" element no longer
//     existed by the time the "after" element was painted (the UI felt sharp).
//   * CSS entrance animations replayed on every unrelated re-render, which is
//     what made the window appear to flash after a press.
//   * Focus, scroll position, text selection and in-progress IME composition
//     were lost, and listeners had to be rebound on each pass.
//
// Morphing patches the existing tree in place instead. Nodes that are still
// present keep their identity, so transitions interpolate normally and
// animations only run for genuinely new nodes. Elements carrying `data-key`
// are matched by key so reordering moves a node rather than rewriting it.
// ---------------------------------------------------------------------------

const isElement = (node: Node): node is HTMLElement => node.nodeType === Node.ELEMENT_NODE;
const isText = (node: Node): node is Text => node.nodeType === Node.TEXT_NODE;

function keyOf(node: Node): string | null {
  return isElement(node) ? node.getAttribute("data-key") : null;
}

function sameKind(a: Node, b: Node): boolean {
  if (a.nodeType !== b.nodeType) return false;
  if (isElement(a) && isElement(b)) return a.tagName === b.tagName;
  return true;
}

/** Copies attributes from `next` onto `current`, removing any that disappeared. */
function patchAttributes(current: HTMLElement, next: HTMLElement): void {
  for (const attribute of Array.from(next.attributes)) {
    if (current.getAttribute(attribute.name) !== attribute.value) {
      current.setAttribute(attribute.name, attribute.value);
    }
  }
  for (const attribute of Array.from(current.attributes)) {
    if (!next.hasAttribute(attribute.name)) current.removeAttribute(attribute.name);
  }

  // Form controls keep their live value: it belongs to the user, not to the
  // rendered markup. Only explicitly declared values are pushed through.
  if (current instanceof HTMLInputElement && next instanceof HTMLInputElement) {
    const declared = next.getAttribute("value");
    if (declared !== null && current.value !== declared && current !== document.activeElement) {
      current.value = declared;
    }
    if (current.checked !== next.hasAttribute("checked")) current.checked = next.hasAttribute("checked");
    current.disabled = next.hasAttribute("disabled");
  }
  if (current instanceof HTMLTextAreaElement && next instanceof HTMLTextAreaElement) {
    current.disabled = next.hasAttribute("disabled");
  }
  if (current instanceof HTMLButtonElement && next instanceof HTMLButtonElement) {
    current.disabled = next.hasAttribute("disabled");
  }
}

function patchNode(current: Node, next: Node): void {
  if (isText(current) && isText(next)) {
    if (current.nodeValue !== next.nodeValue) current.nodeValue = next.nodeValue;
    return;
  }
  if (!isElement(current) || !isElement(next)) return;

  patchAttributes(current, next);

  // A textarea's children are its value. Rewriting them would wipe whatever
  // the user is typing, so the subtree is deliberately left alone.
  if (current.tagName === "TEXTAREA") return;

  // Inline SVG icons are static; skipping them avoids needless namespace work.
  if (current.tagName === "SVG" && current.innerHTML === next.innerHTML) return;

  morphChildren(current, next);
}

function morphChildren(current: Element, next: Element): void {
  const nextNodes = Array.from(next.childNodes);

  // Index the keyed children that already exist so they can be moved instead
  // of being recreated when their position changes.
  const keyed = new Map<string, Element>();
  for (const child of Array.from(current.childNodes)) {
    const key = keyOf(child);
    if (key) keyed.set(key, child as Element);
  }

  for (let index = 0; index < nextNodes.length; index += 1) {
    const wanted = nextNodes[index];
    let existing: Node | null = current.childNodes[index] ?? null;
    const key = keyOf(wanted);

    if (key) {
      const match = keyed.get(key);
      if (match) {
        if (match !== existing) current.insertBefore(match, existing);
        existing = match;
      } else {
        current.insertBefore(wanted.cloneNode(true), existing);
        continue;
      }
    }

    if (!existing) {
      current.appendChild(wanted.cloneNode(true));
      continue;
    }

    // Never reuse a node across keys: a keyed node must only match its key.
    const existingKey = keyOf(existing);
    if (!sameKind(existing, wanted) || (existingKey !== null && existingKey !== key)) {
      current.replaceChild(wanted.cloneNode(true), existing);
      continue;
    }

    patchNode(existing, wanted);
  }

  while (current.childNodes.length > nextNodes.length) {
    current.removeChild(current.childNodes[current.childNodes.length - 1]);
  }
}

/** Patches `target`'s children so they match `html`, preserving live nodes. */
export function morph(target: Element, html: string): void {
  const template = document.createElement("div");
  template.innerHTML = html;
  morphChildren(target, template);
}
