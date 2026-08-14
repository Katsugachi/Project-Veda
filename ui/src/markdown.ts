export function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function inline(value: string): string {
  let rendered = escapeHtml(value);
  rendered = rendered.replace(/`([^`]+)`/g, "<code>$1</code>");
  rendered = rendered.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  rendered = rendered.replace(/(^|[^*])\*([^*\n]+)\*(?!\*)/g, "$1<em>$2</em>");
  rendered = rendered.replace(/\[(S\d+)\]/g, '<strong class="source-n">[$1]</strong>');
  // Links may only point at safe external schemes. Assistant text and the
  // retrieved documentation it quotes are untrusted, so `javascript:` and
  // other dangerous schemes are dropped instead of becoming clickable.
  rendered = rendered.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (match, label: string, url: string) =>
    /^(https?:|mailto:)/i.test(url) ? `<a href="${url}" target="_blank" rel="noopener noreferrer">${label}</a>` : match,
  );
  return rendered;
}

// Renders one paragraph-level block (the text between blank lines).
function renderBlock(part: string): string {
  const lines = part.split("\n");
  const output: string[] = [];
  let paragraph: string[] = [];
  let list: { kind: "ul" | "ol"; items: string[] } | null = null;

  const flushParagraph = () => {
    if (!paragraph.length) return;
    output.push(`<p>${inline(paragraph.join("\n")).replaceAll("\n", "<br>")}</p>`);
    paragraph = [];
  };
  const flushList = () => {
    if (!list) return;
    output.push(
      `<${list.kind}>${list.items.map((item) => `<li>${inline(item)}</li>`).join("")}</${list.kind}>`,
    );
    list = null;
  };

  for (const line of lines) {
    const heading = line.match(/^(#{1,6})\s+(.+)$/);
    if (heading) {
      flushParagraph();
      flushList();
      const level = heading[1].length;
      output.push(`<h${level}>${inline(heading[2])}</h${level}>`);
      continue;
    }
    if (/^(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      flushParagraph();
      flushList();
      output.push("<hr>");
      continue;
    }
    const quote = line.match(/^>\s?(.*)$/);
    if (quote) {
      flushParagraph();
      flushList();
      output.push(`<blockquote>${inline(quote[1])}</blockquote>`);
      continue;
    }
    const unordered = line.match(/^([ \t]*)[-*+]\s+(.*)$/);
    if (unordered) {
      flushParagraph();
      if (!list || list.kind !== "ul") {
        flushList();
        list = { kind: "ul", items: [] };
      }
      list.items.push(unordered[2]);
      continue;
    }
    const ordered = line.match(/^([ \t]*)\d+[.)]\s+(.*)$/);
    if (ordered) {
      flushParagraph();
      if (!list || list.kind !== "ol") {
        flushList();
        list = { kind: "ol", items: [] };
      }
      list.items.push(ordered[2]);
      continue;
    }
    flushList();
    paragraph.push(line);
  }
  flushParagraph();
  flushList();
  return output.join("");
}

export function renderMarkdown(markdown: string): string {
  const blocks: string[] = [];
  // Fenced code blocks are protected first. The replacement is padded with
  // blank lines so it always survives the paragraph split, even when the
  // fences sit directly next to surrounding text.
  const protectedText = markdown.replace(
    /```([\w+-]*)\r?\n([\s\S]*?)```/g,
    (_match, language: string, code: string) => {
      const index =
        blocks.push(
          `<pre data-language="${escapeHtml(language.trim() || "text")}"><code>${escapeHtml(code.trimEnd())}</code></pre>`,
        ) - 1;
      return `\n\n@@VEDA_BLOCK_${index}@@\n\n`;
    },
  );

  return protectedText
    .split(/\r?\n{2,}/)
    .map((part) => part.trim())
    .filter((part) => part.length > 0)
    .map((part) => {
      const marker = part.match(/^@@VEDA_BLOCK_(\d+)@@$/);
      return marker ? blocks[Number(marker[1])] : renderBlock(part);
    })
    .join("");
}
