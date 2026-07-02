#!/usr/bin/env -S npx tsx
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadModel } from './load.ts';
import { validate, type Issue } from './validate.ts';
import { emitDocumentation } from './emit/documentation.ts';
import { emitDocumentationHtml } from './emit/documentation-html.ts';
import { emitViewsMarkdown, emitViewsSql } from './emit/database.ts';
import { emitSchema } from './emit/schema.ts';
import { emitStructurizr, emitMermaid } from './emit/c4.ts';
import { emitTranslationsJson } from './emit/translations.ts';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, '..', '..', '..');

interface Args {
  check: boolean;
  specsDir: string;
  outDir: string;
}

function parseArgs(argv: string[]): Args {
  const args: Args = {
    check: argv.includes('--check'),
    specsDir: join(REPO_ROOT, 'specs'),
    // Committed, canonical generated artifacts live next to the specs that produce them.
    // (tools/codegen/out is now only ephemeral build scratch, e.g. Structurizr .mmd exports.)
    outDir: join(REPO_ROOT, 'specs', 'generated'),
  };
  const get = (flag: string) => {
    const i = argv.indexOf(flag);
    return i !== -1 ? argv[i + 1] : undefined;
  };
  const specs = get('--specs');
  const out = get('--out');
  if (specs) args.specsDir = resolve(specs);
  if (out) args.outDir = resolve(out);
  return args;
}

/** Replace the text between `<!-- GENERATED:<id> START ... -->` and `... END -->` markers in a file. */
function injectGenerated(filePath: string, id: string, body: string): boolean {
  const src = readFileSync(filePath, 'utf8');
  const startRe = new RegExp(`(<!-- GENERATED:${id} START[^>]*-->)`);
  const endRe = new RegExp(`(<!-- GENERATED:${id} END -->)`);
  const startM = src.match(startRe);
  const endM = src.match(endRe);
  if (!startM || !endM || startM.index === undefined || endM.index === undefined) return false;
  const before = src.slice(0, startM.index + startM[0].length);
  const after = src.slice(endM.index);
  writeFileSync(filePath, `${before}\n\n${body}\n\n${after}`, 'utf8');
  return true;
}

function printIssues(issues: Issue[]): void {
  for (const i of issues) {
    const tag = i.level === 'error' ? 'ERROR' : 'warn ';
    console.error(`  [${tag}] ${i.rule}  ${i.location}\n           ${i.message}`);
  }
}

function main(): void {
  const args = parseArgs(process.argv.slice(2));
  console.error(`• specs:  ${args.specsDir}`);

  const model = loadModel(args.specsDir);
  const { report, derived, coverage } = validate(model);

  console.error(
    `• model:  ${model.actors.length} actors, ` +
      `${derived.handledCommands.size} commands, ` +
      `${Object.keys(model.defs['events.yaml']).length} events, ` +
      `${Object.keys(model.defs['errors.yaml']).length} errors`,
  );
  console.error(
    `• api:    ${model.api.mutations.length} mutations, ${model.api.queries.length} queries, ${model.api.types.length} projections`,
  );
  console.error(
    `• stories:${model.personas.length} personas, ${model.personas.reduce((n, p) => n + p.activities.length, 0)} activities`,
  );
  console.error(
    `• views:  ${coverage.views} views, ${coverage.viewColumns} columns, ${coverage.viewFedBy} fedBy links`,
  );
  console.error(
    `• tests:  ${coverage.testCases} behaviour tests, ${Object.keys((model.defs['tests.yaml']?.fixtures ?? {}) as object).length} fixtures, ${coverage.rules} business rules`,
  );
  console.error(
    `• obs:    ${coverage.obsContracts} observability contracts · C4: ${Object.keys((model.defs['architecture/c4-l2.yaml']?.boundedContexts ?? {}) as object).length} bounded contexts`,
  );
  console.error(
    `• ui:     ${coverage.screens} SDUI screens, ${coverage.screenBindings} API bindings, ${coverage.screenGaps} gaps · ${coverage.translations} translation keys (en/fr)`,
  );

  // Make the spec-based validation visible: list what was actually cross-checked.
  console.error('• validated against specs:');
  console.error(`    - ${coverage.refs} $refs resolve (scalars/entities/events/commands/errors/views/api)`);
  console.error('    - actor wiring: messages→commands/events, emits→events, throws→errors');
  console.error(`    - api↔model: ${coverage.mutationLinks} command links→commands, ${coverage.readsLinks} reads→views, roles→UserType`);
  console.error('    - views: aggregate→actors, fedBy→events, column types→scalars, indexes→columns, fk→views');
  console.error(`    - stories: ${coverage.storyLinks} step→op links resolve, persona role authorized, every mutation/query reached by a story step`);
  console.error(`    - tests: ${coverage.testCases} Given/When/Then cases — data fields, actor handles \`when\`, \`then\`⊆emits, \`thrown\`⊆throws; every message/event/error exercised`);
  console.error(`    - rules: ${coverage.rules} business rules — every test asserts ≥1 rule, every rule asserted by ≥1 test (ADR-0032)`);
  console.error(`    - ui: ${coverage.screens} SDUI screens — resolver/action bindings $ref real api ops (API-meets-UI), data_requirements resolve; ${coverage.translations} translations (en+fr, params match)`);
  console.error(`    - observability: ${coverage.obsContracts} workflow contracts — $ref bindings resolve, mandatory ids (correlation_id/trace_id), span kinds, success.required_spans ⊆ declared spans`);
  console.error('    - c4: bounded-context↔actor mapping (no unmapped aggregate / phantom container ref)');

  if (report.issues.length) {
    console.error(`• checks: ${report.errors.length} error(s), ${report.warnings.length} warning(s)`);
    printIssues(report.issues);
  } else {
    console.error('• checks: all cross-references resolve, no warnings');
  }

  if (!report.ok) {
    console.error('\n✗ validation failed — fix the errors above before generating.');
    process.exit(1);
  }

  if (args.check) {
    console.error('\n✓ validation passed (--check: no files written).');
    return;
  }

  mkdirSync(args.outDir, { recursive: true });

  const docTarget = join(args.outDir, 'documentation.generated.md');
  writeFileSync(docTarget, emitDocumentation(model, derived), 'utf8');
  console.error(`\n✓ wrote ${docTarget}`);

  const docHtmlTarget = join(args.outDir, 'documentation.generated.html');
  writeFileSync(
    docHtmlTarget,
    `<!doctype html>\n<html lang="en">\n<head>\n<meta charset="utf-8">\n<meta name="viewport" content="width=device-width, initial-scale=1">\n<title>Captain.Food — Product Documentation</title>\n</head>\n<body>\n${emitDocumentationHtml(model)}\n</body>\n</html>\n`,
    'utf8',
  );
  console.error(`✓ wrote ${docHtmlTarget}`);

  const sqlTarget = join(args.outDir, 'views.generated.sql');
  writeFileSync(sqlTarget, emitViewsSql(model), 'utf8');
  console.error(`✓ wrote ${sqlTarget}`);

  const schemaTarget = join(args.outDir, 'schema.generated.graphql');
  writeFileSync(schemaTarget, emitSchema(model), 'utf8');
  console.error(`✓ wrote ${schemaTarget}`);

  const dslTarget = join(args.outDir, 'c4.generated.dsl');
  writeFileSync(dslTarget, emitStructurizr(model), 'utf8');
  console.error(`✓ wrote ${dslTarget}`);

  const mermaidTarget = join(args.outDir, 'c4.generated.md');
  writeFileSync(mermaidTarget, emitMermaid(model), 'utf8');
  console.error(`✓ wrote ${mermaidTarget}`);

  const i18nTarget = join(args.outDir, 'translations.generated.json');
  writeFileSync(i18nTarget, emitTranslationsJson(model), 'utf8');
  console.error(`✓ wrote ${i18nTarget}`);

  const databaseMd = join(args.specsDir, 'database.md');
  if (injectGenerated(databaseMd, 'views', emitViewsMarkdown(model))) {
    console.error(`✓ injected ${model.views.length} views into ${databaseMd} (between GENERATED:views markers)`);
  } else {
    console.error(`! ${databaseMd}: no GENERATED:views markers found — skipped (add them to enable injection)`);
  }
}

main();
