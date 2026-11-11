/** Small text helpers shared by the CLI and the log parsers. */

export function oneLine(text: string, max = 400): string {
  const flat = text.replace(/\s+/g, ' ').trim();
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat;
}

export function safeJson(value: unknown, max = 300): string {
  try {
    const text = typeof value === 'string' ? value : JSON.stringify(value);
    return text === undefined ? String(value) : oneLine(text, max);
  } catch {
    return String(value);
  }
}
