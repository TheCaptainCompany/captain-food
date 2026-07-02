import type { Model, SchemaNode } from '../model.ts';

/**
 * Emit the single i18n bundle from translations.yaml (ADR-0033): one JSON object mapping each dotted
 * key to its per-locale messages — `{ "<key>": { "en": "…", "fr": "…" } }`. This is the frontend's
 * canonical translation resource (the non-error counterpart of errors.yaml messages).
 */
export function emitTranslationsJson(model: Model): string {
  const defs = (model.defs['translations.yaml'] ?? {}) as Record<string, SchemaNode>;
  const out: Record<string, Record<string, string>> = {};
  for (const key of Object.keys(defs).sort()) {
    const messages = (defs[key]?.messages ?? {}) as Record<string, unknown>;
    const locales: Record<string, string> = {};
    for (const [loc, text] of Object.entries(messages)) locales[loc] = String(text);
    out[key] = locales;
  }
  return JSON.stringify(out, null, 2) + '\n';
}
