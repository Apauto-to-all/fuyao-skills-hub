// 命中行渲染的文本切分：按字符偏移区间把行切成普通段与命中段。

/** spans 命中段与普通段：start 是段首在行内的字符偏移（渲染 key 用它，唯一且稳定） */
type SpanSegment = { start: number; text: string; hit: boolean };

/**
 * 把行文本按字符偏移 spans 切成普通段与命中段。
 * spans 须按产出序有序且互不重叠；越界的 span 按行尾裁剪。
 */
export function splitBySpans(
  text: string,
  spans: [number, number][],
): SpanSegment[] {
  if (spans.length === 0) return [{ start: 0, text, hit: false }];
  const segments: SpanSegment[] = [];
  let cursor = 0;
  for (const [start, end] of spans) {
    const s = Math.max(cursor, start);
    const e = Math.min(text.length, end);
    if (s > cursor)
      segments.push({ start: cursor, text: text.slice(cursor, s), hit: false });
    if (e > s) {
      segments.push({ start: s, text: text.slice(s, e), hit: true });
      cursor = e;
    }
  }
  if (cursor < text.length)
    segments.push({ start: cursor, text: text.slice(cursor), hit: false });
  return segments;
}
