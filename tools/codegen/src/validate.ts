import type { ApiField, Model, SchemaNode, SourceFile } from './model.ts';
import { collectRefs, parseRef, refName, refTargetFile, resolveRef } from './refs.ts';

export interface Issue {
  level: 'error' | 'warning';
  rule: string;
  location: string;
  message: string;
}

export interface ValidationReport {
  issues: Issue[];
  errors: Issue[];
  warnings: Issue[];
  ok: boolean;
}

/** Count of what was actually checked — so a clean run can show coverage, not just silence. */
export interface Coverage {
  refs: number;
  views: number;
  viewColumns: number;
  viewFedBy: number;
  mutationLinks: number;
  readsLinks: number;
  storyLinks: number;
  testCases: number;
  rules: number;
  obsContracts: number;
  translations: number;
  screens: number;
  screenBindings: number;
  screenGaps: number;
}

/**
 * Classification derived purely from references (the model never tags these explicitly):
 *  - a command is "handled" if some actor receives it as a message;
 *  - a commands.yaml entry referenced only from `properties` is a command value object.
 */
export interface Derived {
  handledCommands: Set<string>;
  commandValueObjects: Set<string>;
  unhandledCommands: Set<string>;
  emittedEvents: Set<string>;
  /** Events received as a message by a process-manager (inbound facts / saga triggers). */
  consumedEvents: Set<string>;
  orphanEvents: Set<string>;
}

/** Does `ref` (appearing in `contextFile`) target `file`? Resolves local `#/...` refs. */
function targetsFile(ref: string, file: SourceFile, contextFile: SourceFile): boolean {
  return refTargetFile(ref, contextFile) === file;
}

export function validate(model: Model): { report: ValidationReport; derived: Derived; coverage: Coverage } {
  const issues: Issue[] = [];
  const add = (i: Issue) => issues.push(i);
  const coverage: Coverage = { refs: 0, views: 0, viewColumns: 0, viewFedBy: 0, mutationLinks: 0, readsLinks: 0, storyLinks: 0, testCases: 0, rules: 0, obsContracts: 0, translations: 0, screens: 0, screenBindings: 0, screenGaps: 0 };

  // --- 1. Referential integrity: every `$ref` anywhere must resolve -----------------------------
  for (const file of Object.keys(model.defs) as SourceFile[]) {
    for (const occ of collectRefs(model.defs[file], file)) {
      coverage.refs++;
      const parsed = parseRef(occ.ref);
      if (!parsed) {
        add({ level: 'error', rule: 'ref-format', location: occ.location, message: `Malformed $ref '${occ.ref}'.` });
        continue;
      }
      if (resolveRef(model, occ.ref, file) === null) {
        add({ level: 'error', rule: 'ref-dangling', location: occ.location, message: `$ref '${occ.ref}' does not resolve.` });
      }
    }
  }

  // --- 2. Actor wiring: messages, emits and throws must target the right kind of file ----------
  const handledCommands = new Set<string>();
  const emittedEvents = new Set<string>();
  const consumedEvents = new Set<string>();

  for (const actor of model.actors) {
    actor.receives.forEach((entry, i) => {
      const where = `actors.yaml/${actor.name}.receives[${i}]`;
      const msgRef = entry.message?.$ref;
      if (!msgRef) {
        add({ level: 'error', rule: 'actor-message', location: where, message: 'receives entry has no message $ref.' });
      } else if (targetsFile(msgRef, 'commands.yaml', 'actors.yaml')) {
        const n = refName(msgRef);
        if (n) handledCommands.add(n);
      } else if (targetsFile(msgRef, 'events.yaml', 'actors.yaml')) {
        const n = refName(msgRef);
        if (n) consumedEvents.add(n);
      } else {
        add({ level: 'error', rule: 'actor-message', location: `${where}.message`, message: `message must reference commands.yaml or events.yaml, got '${msgRef}'.` });
      }

      entry.emits.forEach((e, j) => {
        if (!targetsFile(e.$ref, 'events.yaml', 'actors.yaml')) {
          add({ level: 'error', rule: 'actor-emits', location: `${where}.emits[${j}]`, message: `emits must reference events.yaml, got '${e.$ref}'.` });
        } else {
          const n = refName(e.$ref);
          if (n) emittedEvents.add(n);
        }
      });

      entry.throws.forEach((t, j) => {
        if (!targetsFile(t.$ref, 'errors.yaml', 'actors.yaml')) {
          add({ level: 'error', rule: 'actor-throws', location: `${where}.throws[${j}]`, message: `throws must reference errors.yaml, got '${t.$ref}'.` });
        }
      });
    });
  }

  // --- 3. Coverage: derive value-objects vs commands, and orphan events ------------------------
  // commands.yaml entries referenced from `properties` (anywhere) are value objects, not commands.
  const refdFromProperties = new Set<string>();
  for (const file of Object.keys(model.defs) as SourceFile[]) {
    for (const occ of collectRefs(model.defs[file], file)) {
      if (targetsFile(occ.ref, 'commands.yaml', file) && occ.location.includes('.properties.')) {
        const n = refName(occ.ref);
        if (n) refdFromProperties.add(n);
      }
    }
  }

  const allCommands = new Set(Object.keys(model.defs['commands.yaml']));
  const commandValueObjects = new Set<string>();
  const unhandledCommands = new Set<string>();
  for (const c of allCommands) {
    if (handledCommands.has(c)) continue;
    if (refdFromProperties.has(c)) commandValueObjects.add(c);
    else unhandledCommands.add(c);
  }
  for (const c of unhandledCommands) {
    add({ level: 'warning', rule: 'command-unhandled', location: `commands.yaml/${c}`, message: `Command '${c}' is defined but no actor handles it.` });
  }

  const producedEvents = new Set([...emittedEvents, ...consumedEvents]);
  const orphanEvents = new Set<string>();
  for (const e of Object.keys(model.defs['events.yaml'])) {
    if (!producedEvents.has(e)) {
      orphanEvents.add(e);
      add({ level: 'warning', rule: 'event-orphan', location: `events.yaml/${e}`, message: `Event '${e}' is never emitted nor consumed by any actor.` });
    }
  }

  // --- 4. API surface (api.yaml ↔ model) ------------------------------------------------------
  const api = model.api;
  const userTypeSet = new Set((model.defs['scalars.yaml'].UserType?.enum as string[] | undefined) ?? []);
  const INLINE_TYPES = new Set(['string', 'boolean', 'integer', 'float']);
  const allCommandsSet = new Set(Object.keys(model.defs['commands.yaml']));

  const checkRoles = (roles: string[], where: string) => {
    if (roles.length === 0) add({ level: 'error', rule: 'op-no-authz', location: where, message: 'operation declares no roles (→ @auth/@public).' });
    for (const r of roles) if (!userTypeSet.has(r)) add({ level: 'error', rule: 'op-unknown-usertype', location: where, message: `unknown user type '${r}' (not in scalars.yaml#/UserType).` });
  };
  const checkInline = (f: ApiField, where: string) => {
    if (!f.ref && !INLINE_TYPES.has(f.type)) add({ level: 'error', rule: 'api-inline-type', location: where, message: `inline type '${f.type}' must be one of ${[...INLINE_TYPES].join('|')} (or a $ref).` });
  };

  // 4a. mutations: roles, the declared command (defined, handled, dispatched once), payload field types.
  const declaredByCommand = new Map<string, string>(); // command → the mutation that dispatches it
  for (const m of api.mutations) {
    const where = `api.yaml/mutations.${m.name}`;
    checkRoles(m.roles, where);
    if (!m.command) add({ level: 'error', rule: 'op-missing-command', location: where, message: 'mutation declares no command.' });
    else if (!allCommandsSet.has(m.command)) add({ level: 'error', rule: 'mutation-unknown-command', location: where, message: `command '${m.command}' is not defined in commands.yaml.` });
    else if (!handledCommands.has(m.command)) add({ level: 'warning', rule: 'mutation-command-unhandled', location: where, message: `command '${m.command}' has no actor handler.` });
    if (m.command) {
      const prev = declaredByCommand.get(m.command);
      if (prev) add({ level: 'error', rule: 'command-duplicate-mutation', location: where, message: `command '${m.command}' is already dispatched by mutation '${prev}'.` });
      else declaredByCommand.set(m.command, m.name);
    }
    for (const f of m.payload) checkInline(f, `${where}.payload.${f.name}`);
  }
  coverage.mutationLinks = declaredByCommand.size;
  // 4b. every handled command must be dispatched by exactly one mutation.
  for (const cmd of handledCommands) {
    if (!declaredByCommand.has(cmd)) add({ level: 'warning', rule: 'command-no-mutation', location: `commands.yaml/${cmd}`, message: `Handled command '${cmd}' is not dispatched by any mutation.` });
  }

  // 4c. queries: roles, reads present, return type resolves (entities.yaml type or an api projection), arg types.
  const outputTypes = new Set([...Object.keys(model.defs['entities.yaml']), ...api.types.map((t) => t.name)]);
  // Types intentionally NOT view-backed (transient/boundary, e.g. Operation/PaymentIntent): a query
  // returning one is resolver/tracker-served and needs no `@reads`.
  const transientTypes = new Set(api.types.filter((t) => t.reads.length === 0).map((t) => t.name));
  for (const q of api.queries) {
    const where = `api.yaml/queries.${q.name}`;
    checkRoles(q.roles, where);
    if (q.reads.length === 0 && !transientTypes.has(q.returnsType)) add({ level: 'error', rule: 'op-missing-reads', location: where, message: `return type '${q.returnsType || '?'}' declares no \`reads\` binding (→ @reads); bind it to a View_* in api.yaml types.` });
    if (!q.returnsType) add({ level: 'error', rule: 'query-no-returns', location: where, message: 'query has no return type.' });
    else if (!outputTypes.has(q.returnsType)) add({ level: 'error', rule: 'query-unknown-type', location: where, message: `return type '${q.returnsType}' is neither an entities.yaml type nor an api projection.` });
    for (const a of q.args) checkInline(a, `${where}.args.${a.name}`);
  }

  // 4d. subscriptions: roles, return type resolves, arg types. They STREAM — no `@reads` requirement.
  for (const s of api.subscriptions) {
    const where = `api.yaml/subscriptions.${s.name}`;
    checkRoles(s.roles, where);
    if (!s.returnsType) add({ level: 'error', rule: 'subscription-no-returns', location: where, message: 'subscription has no return type.' });
    else if (!outputTypes.has(s.returnsType)) add({ level: 'error', rule: 'subscription-unknown-type', location: where, message: `return type '${s.returnsType}' is neither an entities.yaml type nor an api projection.` });
    for (const a of s.args) checkInline(a, `${where}.args.${a.name}`);
  }

  // --- 5. Read models (views.yaml) ------------------------------------------------------------
  const SQL_PRIMITIVES = new Set(['uuid', 'text', 'integer', 'bigint', 'boolean', 'timestamptz', 'jsonb', 'numeric']);
  const scalarNames = new Set(Object.keys(model.defs['scalars.yaml']));
  const aggregateNames = new Set(model.actors.filter((a) => a.type === 'aggregate').map((a) => a.name));

  coverage.views = model.views.length;
  for (const view of model.views) {
    const at = `views.yaml/${view.name}`;
    coverage.viewColumns += view.columns.length;
    coverage.viewFedBy += view.fedBy.length;
    if (!view.name.startsWith('View_')) add({ level: 'warning', rule: 'view-naming', location: at, message: `Read table '${view.name}' should be prefixed 'View_'.` });
    // A `reference` view is static seed data: no aggregate, no event lineage.
    if (!view.reference && !aggregateNames.has(view.aggregate)) add({ level: 'error', rule: 'view-unknown-aggregate', location: at, message: `aggregate '${view.aggregate}' is not an aggregate in actors.yaml.` });
    if (view.columns.length === 0) add({ level: 'error', rule: 'view-no-columns', location: at, message: 'view has no columns.' });

    const colNames = new Set(view.columns.map((c) => c.name));
    const fedByNames = new Set(view.fedBy.map((r) => refName(r.$ref)).filter((n): n is string => !!n));
    const usedEvents = new Set<string>(); // fedBy events referenced by some column's `from`
    let pkCount = 0;
    for (const col of view.columns) {
      if (col.pk) pkCount++;
      // Column type: declared explicitly OR derived from a `from` event property. Empty = a hole.
      if (!col.type) {
        add({ level: 'error', rule: 'view-column-no-type', location: `${at}.${col.name}`, message: 'column has no `type` and none could be derived from `from` (declare a type or map it to a typed event property).' });
      } else if (!SQL_PRIMITIVES.has(col.type) && !scalarNames.has(col.type)) {
        add({ level: 'error', rule: 'view-column-type', location: `${at}.${col.name}`, message: `type '${col.type}' is neither a SQL primitive nor a scalars.yaml type.` });
      }
      // Lineage (`from`): each source event must be one the view is fed by; a column with no source
      // is a design hole (nothing populates it). Reference views are seed data — no lineage expected.
      if (!col.from || col.from.length === 0) {
        if (!view.reference) add({ level: 'warning', rule: 'view-column-no-source', location: `${at}.${col.name}`, message: 'column has no `from` — not traced to any event (possible design hole).' });
      } else {
        for (const ref of col.from) {
          const ev = refName(ref);
          if (ev && !fedByNames.has(ev)) add({ level: 'error', rule: 'view-column-source-not-fedby', location: `${at}.${col.name}`, message: `from '${ref}' refers to event '${ev}', which is not in this view's fedBy.` });
          if (ev) usedEvents.add(ev);
        }
      }
      if (col.fk) {
        // FK declares read navigation: must point at "View_Name.column" that exists.
        const [fkView, fkCol] = col.fk.split('.');
        const target = model.views.find((v) => v.name === fkView);
        if (!target) add({ level: 'error', rule: 'view-fk-unknown-view', location: `${at}.${col.name}`, message: `fk '${col.fk}' references unknown view '${fkView}'.` });
        else if (!target.columns.some((c) => c.name === fkCol)) add({ level: 'error', rule: 'view-fk-unknown-column', location: `${at}.${col.name}`, message: `fk '${col.fk}' references unknown column '${fkCol}' on '${fkView}'.` });
      }
    }
    if (pkCount === 0) add({ level: 'warning', rule: 'view-no-pk', location: at, message: 'view declares no primary-key column.' });

    view.fedBy.forEach((r, i) => {
      const n = refName(r.$ref);
      if (n && !producedEvents.has(n)) add({ level: 'warning', rule: 'view-fedby-unproduced', location: `${at}.fedBy[${i}]`, message: `fed by '${n}', which no actor emits or consumes.` });
    });
    view.indexes.forEach((ix, i) => {
      for (const c of ix) {
        if (!colNames.has(c)) add({ level: 'error', rule: 'view-index-column', location: `${at}.indexes[${i}]`, message: `index references unknown column '${c}'.` });
      }
    });
    // A fedBy event that no column maps `from` is consumed for nothing here (possible design hole),
    // unless no column on the view declares `from` yet (lineage not annotated at all → skip the noise).
    if ([...usedEvents].length) {
      for (const ev of fedByNames) {
        if (!usedEvents.has(ev)) add({ level: 'warning', rule: 'view-fedby-unused', location: `${at}`, message: `fed by '${ev}' but no column maps \`from\` it (possible design hole).` });
      }
    }
  }

  // 5b. every emitted event should be projected into a view, unless declared non-projected (transient).
  const nonProjected = new Set(model.nonProjectedEvents);
  for (const e of emittedEvents) {
    if (nonProjected.has(e)) continue;
    if (!model.views.some((v) => v.fedBy.some((r) => refName(r.$ref) === e))) {
      add({ level: 'warning', rule: 'event-not-projected', location: `events.yaml/${e}`, message: `Emitted event '${e}' feeds no View_* (mark it under views.yaml nonProjectedEvents if intentional).` });
    }
  }

  // 5c. type `reads` (api.yaml) bind output types to views: every bound view must exist, and every
  // non-internal view must be bound by some output type (a type is a resolver — reachable via a query
  // directly or by FK navigation). Reads live on the TYPE; queries inherit their return type's binding.
  {
    const viewNames = new Set(model.views.map((v) => v.name));
    const internalViews = new Set(model.views.filter((v) => v.internal).map((v) => v.name));
    const boundViews = new Set<string>();
    for (const t of api.types) {
      for (const v of t.reads) {
        coverage.readsLinks++;
        boundViews.add(v);
        if (!viewNames.has(v)) add({ level: 'error', rule: 'reads-unknown-view', location: `api.yaml/types.${t.name}`, message: `reads references unknown view '${v}'.` });
      }
    }
    for (const v of viewNames) {
      // internal views are read by command handlers / auth, not by a GraphQL query — exempt.
      if (!boundViews.has(v) && !internalViews.has(v)) add({ level: 'warning', rule: 'view-no-query', location: `views.yaml/${v}`, message: `View '${v}' is bound by no output type (api.yaml types reads).` });
    }
  }

  // --- 6. Story map (stories.yaml): personas → activities → steps -----------------------------
  // Every step references an existing api op, and the persona's role may actually call it
  // (op is @public, i.e. roles include PUBLIC, OR the persona's role is in the op's roles).
  {
    const queryRoles = new Map(api.queries.map((q) => [q.name, q.roles]));
    const mutationRoles = new Map(api.mutations.map((m) => [m.name, m.roles]));
    for (const p of model.personas) {
      const at = `stories.yaml/${p.name}`;
      if (!p.role) add({ level: 'error', rule: 'persona-no-role', location: at, message: 'persona declares no personaRole.' });
      else if (!userTypeSet.has(p.role)) add({ level: 'error', rule: 'persona-unknown-role', location: at, message: `personaRole '${p.role}' is not a scalars.yaml#/UserType.` });
      for (const act of p.activities) {
        for (const step of act.steps) {
          if (!step.op || !step.opKind) continue; // note-only step
          coverage.storyLinks++;
          const where = `${at}.${act.name}.${step.name}`;
          const roles = step.opKind === 'query' ? queryRoles.get(step.op) : mutationRoles.get(step.op);
          if (!roles) { add({ level: 'error', rule: 'story-unknown-op', location: where, message: `step references unknown ${step.opKind} '${step.op}'.` }); continue; }
          const allowed = roles.includes('PUBLIC') || (p.role !== '' && roles.includes(p.role));
          if (!allowed) add({ level: 'error', rule: 'story-role-not-authorized', location: where, message: `persona role '${p.role}' may not call ${step.opKind} '${step.op}' (op roles: [${roles.join(', ')}]).` });
        }
      }
    }

    // COMPLETENESS (the reverse): every mutation & query must be reached by ≥1 story step, so the API
    // surface stays anchored to a persona use case (subscriptions are a transport variant of a query —
    // the story step model only carries query/mutation — and are exempt).
    const storyOps = new Set<string>();
    for (const p of model.personas) for (const act of p.activities) for (const step of act.steps) if (step.op) storyOps.add(step.op);
    for (const m of api.mutations) if (!storyOps.has(m.name)) add({ level: 'error', rule: 'op-uncovered-by-story', location: `api.yaml/mutations/${m.name}`, message: `mutation '${m.name}' is referenced by no story step (stories.yaml) — every write must anchor to a persona use case.` });
    for (const q of api.queries) if (!storyOps.has(q.name)) add({ level: 'error', rule: 'op-uncovered-by-story', location: `api.yaml/queries/${q.name}`, message: `query '${q.name}' is referenced by no story step (stories.yaml) — every read must anchor to a persona use case.` });
  }

  // --- 7. Behaviour tests (tests.yaml): centralized fixtures + Given/When/Then consistency -----
  // Every `$ref` already resolves (§1). Here we check the SEMANTICS against the actor model:
  //  - a fixture's/command's `data` keys exist on its event/command schema (no typos, no stale fields);
  //  - the test's actor actually HANDLES the `when` command (actors.yaml);
  //  - each `then` fixture's event is one that handler EMITS for that command.
  {
    const testsFile = (model.defs['tests.yaml'] ?? {}) as Record<string, Record<string, SchemaNode>>;
    const fixtures = (testsFile.fixtures ?? {}) as Record<string, SchemaNode>;
    const tests = (testsFile.tests ?? {}) as Record<string, SchemaNode>;

    // Recursively check a data value against its schema node: every REQUIRED property must be set,
    // and no UNKNOWN field may appear. Recurses through `$ref`s (value objects), `properties` (inline
    // objects) and `array` items, so nested shapes (Money, Address, Offer…) are checked too. We only
    // descend into keys actually present in the data, so a (possibly cyclic) schema can't loop.
    const checkShape = (node: SchemaNode | null, data: unknown, where: string) => {
      if (!node) return;
      if (typeof node.$ref === 'string') { checkShape(resolveRef(model, node.$ref, 'tests.yaml'), data, where); return; }
      const props = node.properties as Record<string, SchemaNode> | undefined;
      if (props) {
        const required = Array.isArray(node.required) ? (node.required as string[]) : [];
        const obj = data && typeof data === 'object' && !Array.isArray(data) ? (data as Record<string, unknown>) : undefined;
        for (const r of required) {
          if (!obj || !(r in obj)) add({ level: 'error', rule: 'test-missing-required', location: `${where}.${r}`, message: `required property '${r}' is not set by the data.` });
        }
        if (obj) {
          for (const [k, v] of Object.entries(obj)) {
            if (!(k in props)) add({ level: 'error', rule: 'test-unknown-field', location: `${where}.${k}`, message: `data field '${k}' is not a property of this schema.` });
            else checkShape(props[k] ?? null, v, `${where}.${k}`);
          }
        }
        return;
      }
      if (node.type === 'array' && node.items && Array.isArray(data)) {
        data.forEach((item, i) => checkShape(node.items as SchemaNode, item, `${where}[${i}]`));
      }
      // otherwise a leaf (scalar / primitive) — nothing to check.
    };
    const checkData = (typeRef: string, data: unknown, where: string) =>
      checkShape(resolveRef(model, typeRef, 'tests.yaml'), data, where);
    // the event name a `#/fixtures/<name>` ref ultimately denotes.
    const fixtureEvent = (fxRef: unknown): string | null => {
      if (typeof fxRef !== 'string') return null;
      const fx = resolveRef(model, fxRef, 'tests.yaml') as Record<string, SchemaNode> | null;
      const typeRef = (fx?.type as { $ref?: string } | undefined)?.$ref;
      return typeof typeRef === 'string' ? refName(typeRef) : null;
    };

    // Per-actor INBOX: each (actor, message) entry → what it emits / may throw. `message` is a command
    // (aggregate handler) OR an event (process-manager reaction), so a test's `when` may be either.
    type InboxEntry = { actor: string; message: string; isCommand: boolean; emits: Set<string>; throws: Set<string> };
    const inbox = new Map<string, Map<string, InboxEntry>>();
    const inboxEntries: InboxEntry[] = [];
    const emittedEvents = new Set<string>();   // every event some actor emits → must be asserted in a `then`
    const throwableErrors = new Set<string>(); // every error some handler may throw → must be asserted in a `thrown`
    for (const a of model.actors) {
      const byMsg = new Map<string, InboxEntry>();
      for (const e of a.receives) {
        const msg = refName(e.message?.$ref ?? '');
        if (!msg) continue;
        const entry: InboxEntry = {
          actor: a.name,
          message: msg,
          isCommand: (e.message?.$ref ?? '').startsWith('commands.yaml#/'),
          emits: new Set(e.emits.map((r) => refName(r.$ref)).filter((n): n is string => !!n)),
          throws: new Set(e.throws.map((r) => refName(r.$ref)).filter((n): n is string => !!n)),
        };
        byMsg.set(msg, entry);
        inboxEntries.push(entry);
        entry.emits.forEach((ev) => emittedEvents.add(ev));
        entry.throws.forEach((er) => throwableErrors.add(er));
      }
      inbox.set(a.name, byMsg);
    }

    // What the test suite actually exercises (for coverage detection below).
    const usedMessages = new Set<string>(); // `${actor}::${message}`
    const usedEvents = new Set<string>();   // events appearing in a given/then, or as an event `when`
    const usedErrors = new Set<string>();   // errors appearing in a `thrown`
    const usedRules = new Set<string>();    // rules.yaml rules asserted by ≥1 test (via `rules`)
    const allRules = Object.keys((model.defs['rules.yaml'] ?? {}) as Record<string, SchemaNode>);
    coverage.rules = allRules.length;

    // 7a. fixtures: data shape.
    for (const [name, fx] of Object.entries(fixtures)) {
      const where = `tests.yaml/fixtures.${name}`;
      const ref = (fx?.type as { $ref?: string } | undefined)?.$ref;
      if (typeof ref !== 'string') { add({ level: 'error', rule: 'fixture-no-type', location: where, message: 'fixture has no `type.$ref`.' }); continue; }
      checkData(ref, fx.data, where);
    }

    // 7b. tests: the actor handles the `when` message; `then` ⊆ emits; `thrown` ⊆ throws; data shapes.
    coverage.testCases = Object.keys(tests).length;
    for (const [name, t] of Object.entries(tests)) {
      const where = `tests.yaml/tests.${name}`;
      const actorName = refName((t?.actor as { $ref?: string } | undefined)?.$ref ?? '');
      const when = t?.when as { type?: { $ref?: string }; data?: unknown } | undefined;
      const whenRef = when?.type?.$ref;
      if (typeof whenRef !== 'string') { add({ level: 'error', rule: 'test-no-when', location: where, message: 'test has no `when.type.$ref` (command or event).' }); continue; }
      checkData(whenRef, when?.data, `${where}.when`);

      const msg = refName(whenRef) ?? '';
      const entry = actorName && msg ? inbox.get(actorName)?.get(msg) : undefined;
      if (!entry) add({ level: 'error', rule: 'test-message-not-handled', location: `${where}.when`, message: `actor '${actorName}' does not receive '${msg}' (actors.yaml inbox).` });
      else {
        usedMessages.add(`${actorName}::${msg}`);
        if (!entry.isCommand) usedEvents.add(msg); // an event `when` (process-manager reaction) exercises that event
      }

      // `given` preconditions exercise their events too.
      (Array.isArray(t?.given) ? (t.given as Array<{ $ref?: string }>) : []).forEach((g) => {
        const ev = fixtureEvent(g?.$ref); if (ev) usedEvents.add(ev);
      });

      // Every test must assert ≥1 business rule (rules.yaml) — the readable intent it verifies (ADR-0032).
      const testRules = Array.isArray(t?.rules) ? (t.rules as Array<{ $ref?: string }>) : [];
      if (testRules.length === 0) add({ level: 'error', rule: 'test-no-rule', location: where, message: 'test asserts no business rule — add `rules: [{ $ref: \'rules.yaml#/<Rule>\' }]` (ADR-0032).' });
      testRules.forEach((r, i) => {
        if (refTargetFile(r?.$ref ?? '', 'tests.yaml') !== 'rules.yaml') { add({ level: 'error', rule: 'test-rule-wrong-file', location: `${where}.rules[${i}]`, message: `rule ref '${r?.$ref}' must target rules.yaml.` }); return; }
        const rn = refName(r.$ref ?? ''); if (rn) usedRules.add(rn); // resolution itself is checked in §1
      });

      // A test must assert SOMETHING: `then` (events emitted — possibly [] for an idempotent no-op)
      // and/or `thrown` (the message is rejected with one of these errors).
      const hasThen = Object.prototype.hasOwnProperty.call(t, 'then');
      const hasThrown = Object.prototype.hasOwnProperty.call(t, 'thrown');
      if (!hasThen && !hasThrown) add({ level: 'error', rule: 'test-no-assertion', location: where, message: 'test asserts nothing — declare `then` (events, [] for a no-op) and/or `thrown` (errors).' });

      const thens = Array.isArray(t?.then) ? (t.then as Array<{ $ref?: string }>) : [];
      thens.forEach((th, i) => {
        const ev = fixtureEvent(th?.$ref);
        if (!ev) return;
        usedEvents.add(ev);
        if (entry && !entry.emits.has(ev)) add({ level: 'error', rule: 'test-then-not-emitted', location: `${where}.then[${i}]`, message: `expected event '${ev}' is not emitted by '${entry.actor}' for '${msg}'.` });
      });

      // `thrown` lists the error(s) the rejection may raise — each must be one the handler DECLARES it
      // throws for this message (actors.yaml), the rejection mirror of `then` ⊆ emits.
      const throwns = Array.isArray(t?.thrown) ? (t.thrown as Array<{ $ref?: string }>) : [];
      throwns.forEach((th, i) => {
        const err = typeof th?.$ref === 'string' ? refName(th.$ref) : null;
        if (!err) return;
        usedErrors.add(err);
        if (entry && !entry.throws.has(err)) add({ level: 'error', rule: 'test-thrown-not-declared', location: `${where}.thrown[${i}]`, message: `error '${err}' is not declared in '${entry.actor}' throws for '${msg}' (actors.yaml).` });
      });
    }

    // 7c. COVERAGE (BLOCKING — the spec is not complete until every reachable item is exercised):
    // every actor message, emitted event and throwable error must appear in a test, AND every business
    // rule must be asserted by a test (the reverse of §7b's test→rule check). Both directions are errors
    // so tests/rules can't silently drift from the model (ADR-0032).
    for (const e of inboxEntries) {
      if (!usedMessages.has(`${e.actor}::${e.message}`)) add({ level: 'error', rule: 'test-uncovered-message', location: `actors.yaml/${e.actor}`, message: `no test exercises ${e.isCommand ? 'command' : 'event'} '${e.message}' on '${e.actor}'.` });
    }
    for (const ev of emittedEvents) {
      if (!usedEvents.has(ev)) add({ level: 'error', rule: 'test-uncovered-event', location: `events.yaml/${ev}`, message: `emitted event '${ev}' is asserted by no test (in a \`then\`/\`given\`).` });
    }
    for (const er of throwableErrors) {
      if (!usedErrors.has(er)) add({ level: 'error', rule: 'test-uncovered-error', location: `errors.yaml/${er}`, message: `throwable error '${er}' is asserted by no test (in a \`thrown\`).` });
    }
    for (const rn of allRules) {
      if (!usedRules.has(rn)) add({ level: 'error', rule: 'rule-uncovered', location: `rules.yaml/${rn}`, message: `business rule '${rn}' is asserted by no test — add a test with \`rules: [{ $ref: 'rules.yaml#/${rn}' }]\` or remove the rule (ADR-0032).` });
    }
  }

  // --- 8. Observability contracts (observability.yaml) -----------------------------------------
  // Each critical-workflow contract is checked for the mandatory shape (every `$ref` binding already
  // resolved in §1): the mandatory run identifiers, well-formed spans, and a coherent success rule.
  {
    const SPAN_KINDS = new Set(['SERVER', 'CLIENT', 'INTERNAL', 'PRODUCER', 'CONSUMER']);
    const obs = (model.defs['observability.yaml'] ?? {}) as Record<string, SchemaNode>;
    for (const [feature, raw] of Object.entries(obs)) {
      const c = raw as Record<string, unknown>;
      const at = `observability.yaml/${feature}`;
      coverage.obsContracts++;

      // workflow must bind to the domain (a command and/or a saga/aggregate).
      const wf = (c.workflow ?? {}) as Record<string, unknown>;
      if (!wf.command && !wf.saga && !wf.aggregate) add({ level: 'error', rule: 'obs-no-workflow-binding', location: at, message: 'workflow must bind a `command` and/or `saga`/`aggregate` ($ref into the model).' });

      // mandatory run identifiers: correlation_id (business, whole chain) + trace_id (technical).
      const ids = Array.isArray(c.run_identity) ? (c.run_identity as Array<Record<string, unknown>>) : [];
      const idNames = new Set(ids.map((i) => i?.name));
      for (const must of ['correlation_id', 'trace_id']) {
        if (!idNames.has(must)) add({ level: 'error', rule: 'obs-missing-id', location: `${at}.run_identity`, message: `run_identity must declare the mandatory id '${must}'.` });
      }

      // spans: at least one; each with a name and a valid OTel kind.
      const spans = Array.isArray(c.spans) ? (c.spans as Array<Record<string, unknown>>) : [];
      if (!spans.length) add({ level: 'error', rule: 'obs-no-spans', location: at, message: 'contract declares no spans.' });
      const spanNames = new Set<string>();
      spans.forEach((s, i) => {
        if (typeof s?.name !== 'string') add({ level: 'error', rule: 'obs-span-no-name', location: `${at}.spans[${i}]`, message: 'span has no `name`.' });
        else spanNames.add(s.name);
        if (typeof s?.kind === 'string' && !SPAN_KINDS.has(s.kind)) add({ level: 'error', rule: 'obs-span-kind', location: `${at}.spans[${i}]`, message: `span kind '${s.kind}' is not one of ${[...SPAN_KINDS].join('|')}.` });
      });

      // status_rules.success.required_spans must be a subset of the declared spans.
      const success = ((c.status_rules ?? {}) as Record<string, unknown>).success as Record<string, unknown> | undefined;
      const reqSpans = Array.isArray(success?.required_spans) ? (success!.required_spans as string[]) : [];
      for (const rs of reqSpans) {
        if (!spanNames.has(rs)) add({ level: 'error', rule: 'obs-required-span-undeclared', location: `${at}.status_rules.success`, message: `required_span '${rs}' is not a declared span.` });
      }
    }
  }

  // --- 9. C4 consistency (architecture/c4-l2.yaml): every actor is mapped to a bounded context -----
  {
    const l2 = (model.defs['architecture/c4-l2.yaml'] ?? {}) as Record<string, SchemaNode>;
    const bcs = (l2.boundedContexts ?? {}) as Record<string, Record<string, unknown>>;
    const mapped = new Set<string>();
    for (const bc of Object.values(bcs)) {
      for (const ref of [...(Array.isArray(bc.aggregates) ? bc.aggregates : []), ...(Array.isArray(bc.processManagers) ? bc.processManagers : [])]) {
        const n = refName((ref as { $ref?: string })?.$ref ?? '');
        if (n) mapped.add(n);
      }
    }
    if (Object.keys(bcs).length) {
      for (const a of model.actors) {
        if (!mapped.has(a.name)) add({ level: 'warning', rule: 'c4-actor-unmapped', location: 'architecture/c4-l2.yaml', message: `actor '${a.name}' belongs to no bounded context (C4 L2 drift).` });
      }
    }
    // A context's optional `roles` (UserType performers, driving op grouping in the docs) must be valid
    // UserType values, and each UserType maps to at most one context (no overlapping ownership).
    const userTypes = new Set(((model.defs['scalars.yaml']?.UserType as { enum?: string[] })?.enum) ?? []);
    const roleOwner = new Map<string, string>();
    for (const [cid, bc] of Object.entries(bcs)) {
      for (const role of (Array.isArray(bc.roles) ? bc.roles : [])) {
        const r = String(role);
        if (userTypes.size && !userTypes.has(r)) add({ level: 'error', rule: 'c4-context-role-unknown', location: `architecture/c4-l2.yaml/${cid}`, message: `bounded-context role '${r}' is not a scalars.yaml#/UserType value.` });
        const prev = roleOwner.get(r);
        if (prev && prev !== cid) add({ level: 'error', rule: 'c4-context-role-overlap', location: `architecture/c4-l2.yaml/${cid}`, message: `UserType '${r}' is claimed by both '${prev}' and '${cid}' — each role maps to at most one context.` });
        else roleOwner.set(r, cid);
      }
    }
  }

  // --- 10. Translations (translations.yaml): every key has en+fr; placeholders match declared params ---
  // The non-error i18n catalog (ADR-0033). `{param}` tokens in en/fr must reference a declared `params`
  // name, and both locales must use exactly the declared param set (no drift, no undocumented tokens).
  {
    const tr = (model.defs['translations.yaml'] ?? {}) as Record<string, SchemaNode>;
    const placeholders = (s: unknown): Set<string> => {
      const out = new Set<string>();
      if (typeof s === 'string') for (const m of s.matchAll(/\{(\w+)\}/g)) out.add(m[1]!);
      return out;
    };
    for (const [key, raw] of Object.entries(tr)) {
      const t = raw as Record<string, unknown>;
      const at = `translations.yaml/${key}`;
      coverage.translations++;
      const messages = (t.messages ?? {}) as Record<string, unknown>;
      for (const loc of ['en', 'fr']) {
        if (typeof messages[loc] !== 'string' || !(messages[loc] as string).length)
          add({ level: 'error', rule: 'translation-missing-locale', location: at, message: `translation '${key}' has no '${loc}' message (both en and fr are required).` });
      }
      const params = new Set(Object.keys((t.params ?? {}) as Record<string, unknown>));
      for (const loc of ['en', 'fr']) {
        for (const ph of placeholders(messages[loc])) {
          if (!params.has(ph)) add({ level: 'error', rule: 'translation-param-mismatch', location: at, message: `'${loc}' message uses {${ph}} but it is not declared in \`params\`.` });
        }
      }
      // every declared param must be used by at least one locale (no dead params).
      const used = new Set<string>([...placeholders(messages.en), ...placeholders(messages.fr)]);
      for (const p of params) if (!used.has(p)) add({ level: 'error', rule: 'translation-param-mismatch', location: at, message: `declared param '${p}' is used by no message.` });
    }
  }

  // --- 11. Customer screens (customer_screens.yaml): the SDUI spec is bound to the API (ADR-0033) -----
  // Every resolver/action `$ref` already resolved in §1 (a screen calling a non-existent op → ref-dangling:
  // the API-meets-UI gate). Here: resolver/action bindings target the right api op-kind, each screen's
  // data_requirements name a declared resolver, and we count screens/bindings/gaps for the summary.
  {
    const cs = (model.defs['customer_screens.yaml'] ?? {}) as Record<string, SchemaNode>;
    const resolvers = (cs.resolvers ?? {}) as Record<string, Record<string, unknown>>;
    const actions = (cs.actions ?? {}) as Record<string, Record<string, unknown>>;
    const queryNames = new Set(api.queries.map((q) => q.name));
    const mutationNames = new Set(api.mutations.map((m) => m.name));

    // op refs are nested (api.yaml#/queries/<name>) so the op name is the LAST path segment.
    const opName = (ref: string): string => ref.split('/').pop() ?? '';
    for (const [name, r] of Object.entries(resolvers)) {
      if (r?.gap) { coverage.screenGaps++; continue; }
      const ref = (r?.query as { $ref?: string } | undefined)?.$ref;
      if (!ref) { add({ level: 'error', rule: 'resolver-no-binding', location: `customer_screens.yaml/resolvers/${name}`, message: `resolver '${name}' must declare a \`query\` ($ref into api.yaml) or a \`gap\`.` }); continue; }
      if (refTargetFile(ref, 'customer_screens.yaml') !== 'api.yaml' || !ref.includes('/queries/') || !queryNames.has(opName(ref)))
        add({ level: 'error', rule: 'resolver-not-a-query', location: `customer_screens.yaml/resolvers/${name}`, message: `resolver '${name}' query must $ref an api.yaml query; '${ref}' is not one.` });
      else coverage.screenBindings++;
    }
    for (const [name, a] of Object.entries(actions)) {
      const ref = (a?.mutation as { $ref?: string } | undefined)?.$ref;
      if (!ref) continue; // client/auth/gap actions carry no mutation binding
      if (refTargetFile(ref, 'customer_screens.yaml') !== 'api.yaml' || !ref.includes('/mutations/') || !mutationNames.has(opName(ref)))
        add({ level: 'error', rule: 'action-not-a-mutation', location: `customer_screens.yaml/actions/${name}`, message: `action '${name}' mutation must $ref an api.yaml mutation; '${ref}' is not one.` });
      else coverage.screenBindings++;
    }
    const screens = Array.isArray(cs.screens) ? (cs.screens as Array<Record<string, unknown>>) : [];
    for (const s of screens) {
      coverage.screens++;
      const sid = String(s?.id ?? '?');
      coverage.screenGaps += Array.isArray(s?.gaps) ? (s.gaps as unknown[]).length : 0;
      for (const dr of (Array.isArray(s?.data_requirements) ? s.data_requirements : []) as unknown[]) {
        if (!resolvers[String(dr)]) add({ level: 'error', rule: 'screen-unknown-resolver', location: `customer_screens.yaml/screens/${sid}`, message: `data_requirement '${dr}' is not a declared resolver.` });
      }
    }
  }

  const errors = issues.filter((i) => i.level === 'error');
  const warnings = issues.filter((i) => i.level === 'warning');
  return {
    report: { issues, errors, warnings, ok: errors.length === 0 },
    derived: { handledCommands, commandValueObjects, unhandledCommands, emittedEvents, consumedEvents, orphanEvents },
    coverage,
  };
}
