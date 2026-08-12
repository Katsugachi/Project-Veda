export function escapeHtml(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#039;");
}

function inline(value: string): string {
  return escapeHtml(value)
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/\[(S\d+)\]/g, '<strong class="source-n">[$1]</strong>');
}

export function renderMarkdown(markdown: string): string {
  const blocks: string[] = [];
  const protectedText = markdown.replace(/```([\w+-]*)\n([\s\S]*?)```/g, (_match, lang: string, code: string) => {
    const index = blocks.push(`<pre data-language="${escapeHtml(lang || "text")}"><code>${escapeHtml(code.trimEnd())}</code></pre>`) - 1;
    return `\n@@PALOR_BLOCK_${index}@@\n`;
  });

  return protectedText
    .split(/\n{2,}/)
    .map((part) => {
      const marker = part.trim().match(/^@@PALOR_BLOCK_(\d+)@@$/);
      if (marker) return blocks[Number(marker[1])];
      return `<p>${inline(part.trim()).replaceAll("\n", "<br>")}</p>`;
    })
    .join("");
}
