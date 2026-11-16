type Fence = {
  char: string;
  length: number;
};

export type MarkdownBlock = {
  content: string;
  open: boolean;
};

const fenceStart = /^ {0,3}(`{3,}|~{3,})/;
const listItem = /^\s*(?:[-*+]|\d{1,9}[.)])\s+/;
const quoteLine = /^\s*>/;

export function splitMarkdownBlocks(source: string, streaming: boolean): MarkdownBlock[] {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  const blocks: string[] = [];
  let current: string[] = [];
  let fence: Fence | null = null;
  let math = false;

  function flush() {
    while (current.length > 0 && (current.at(-1) ?? "").trim() === "") {
      current.pop();
    }

    const block = current.join("\n");
    const previous = blocks.at(-1);

    if (block.length > 0 && previous !== undefined && continuesPrevious(previous, block)) {
      blocks[blocks.length - 1] = `${previous}\n\n${block}`;
    } else if (block.length > 0) {
      blocks.push(block);
    }

    current = [];
  }

  for (const line of lines) {
    if (fence) {
      current.push(line);

      const close = /^ {0,3}(`{3,}|~{3,})\s*$/.exec(line);
      const marker = close?.[1] ?? "";

      if (marker.startsWith(fence.char) && marker.length >= fence.length) {
        fence = null;
      }

      continue;
    }

    if (math) {
      current.push(line);

      if (line.trim() === "$$") {
        math = false;
      }

      continue;
    }

    const trimmed = line.trim();

    if (trimmed === "") {
      flush();
      continue;
    }

    const opening = fenceStart.exec(line);

    if (opening) {
      const marker = opening[1] ?? "```";

      fence = { char: marker[0] ?? "`", length: marker.length };
      current.push(line);
      continue;
    }

    if (trimmed.startsWith("$$")) {
      current.push(line);

      if (!(trimmed.length > 4 && trimmed.endsWith("$$"))) {
        math = true;
      }

      continue;
    }

    current.push(line);
  }

  flush();

  return blocks.map((content, index) => ({
    content,
    open: streaming && index === blocks.length - 1 && !isStructurallyClosed(content),
  }));
}

function isStructurallyClosed(block: string): boolean {
  const lines = block.split("\n").filter((line) => line.trim() !== "");
  const first = lines[0];
  const last = lines.at(-1);

  if (!first || !last || lines.length < 2) {
    return false;
  }

  const opening = fenceStart.exec(first);

  if (opening) {
    const marker = opening[1] ?? "";
    const closing = /^ {0,3}(`{3,}|~{3,})\s*$/.exec(last)?.[1] ?? "";

    return closing.startsWith(marker[0] ?? "`") && closing.length >= marker.length;
  }

  return first.trim().startsWith("$$") && last.trim() === "$$";
}

function continuesPrevious(previous: string, next: string): boolean {
  if (listItem.test(previous) && listItem.test(next)) {
    return true;
  }

  return quoteLine.test(previous) && quoteLine.test(next);
}
