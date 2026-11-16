const matchStart = "\u0002";
const matchEnd = "\u0003";

export const minimumSearchQuery = 2;
export const searchResultLimit = 40;

export type SnippetPart = {
  text: string;
  highlighted: boolean;
};

export function snippetParts(snippet: string): SnippetPart[] {
  const parts: SnippetPart[] = [];
  let cursor = 0;

  while (cursor < snippet.length) {
    const start = snippet.indexOf(matchStart, cursor);

    if (start === -1) {
      parts.push({ text: snippet.slice(cursor), highlighted: false });
      break;
    }

    if (start > cursor) {
      parts.push({ text: snippet.slice(cursor, start), highlighted: false });
    }

    const end = snippet.indexOf(matchEnd, start + 1);

    if (end === -1) {
      parts.push({ text: snippet.slice(start + 1), highlighted: true });
      break;
    }

    parts.push({ text: snippet.slice(start + 1, end), highlighted: true });
    cursor = end + 1;
  }

  return parts.filter((part) => part.text.length > 0);
}
