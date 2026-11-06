// SPDX-License-Identifier: Apache-2.0
// Chinese (and Japanese, Korean) keyword search for memories. SQLite's default FTS5 tokenizer keeps
// a whole run of CJK characters as one token, and OmniRoute's query sanitizer drops CJK characters
// altogether, so a Chinese query never matched anything. Node's built-in ICU segmenter splits the
// query into words, which are then matched as substrings; personal memory tables are small.
const CJK = /[぀-ヿ㐀-䶿一-鿿豈-﫿가-힯]/;
const segmenter = new Intl.Segmenter("zh", { granularity: "word" });

export const hasCjk = (text?: string | null) => !!text && CJK.test(text);

/** Distinct search words: CJK words of two or more characters, other words of three or more. */
export function cjkTerms(query: string, max = 8): string[] {
  const out = new Set<string>();
  for (const part of segmenter.segment(query.slice(0, 500))) {
    if (!part.isWordLike) continue;
    const word = part.segment.trim().toLowerCase();
    if (!word || (CJK.test(word) ? word.length < 2 : word.length < 3)) continue;
    out.add(word);
    if (out.size >= max) break;
  }
  return [...out];
}

const escapeLike = (term: string) => term.replace(/[\\%_]/g, (c) => "\\" + c);

/** A WHERE fragment matching any term in the memory's content or key, with its parameters. */
export function cjkLikeClause(alias: string, terms: string[]) {
  const sql = terms
    .map(() => `(${alias}.content LIKE ? ESCAPE '\\' OR ${alias}.key LIKE ? ESCAPE '\\')`)
    .join(" OR ");
  const params = terms.flatMap((term) => {
    const pattern = `%${escapeLike(term)}%`;
    return [pattern, pattern];
  });
  return { sql: `(${sql})`, params };
}

/** Rows matching more of the terms first; ties keep their order. */
export function rankByTerms<T extends { content?: unknown; key?: unknown }>(rows: T[], terms: string[]): T[] {
  const score = (row: T) => {
    const text = `${row.content ?? ""} ${row.key ?? ""}`.toLowerCase();
    return terms.filter((term) => text.includes(term)).length;
  };
  return rows
    .map((row, index) => ({ row, index, score: score(row) }))
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .map((entry) => entry.row);
}
