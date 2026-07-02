/**
 * In-memory shape of the Captain.Food spec model.
 *
 * The generator never hard-codes domain knowledge: everything below is loaded from `specs/*.yaml`.
 * A "definition" is a top-level named entry in one of the source files. The files also carry
 * file-level metadata (`version`, `description`) which is NOT a definition and is stripped on load.
 */

/** A JSON-Schema-ish node as written in the yaml (kept loose on purpose — we only walk `$ref`s). */
export type SchemaNode = Record<string, unknown>;

/** A `$ref` object, e.g. `{ $ref: 'scalars.yaml#/RestaurantId' }`. */
export interface RefNode {
  $ref: string;
}

/** The source files we treat as the canonical model, in load order. */
export const SOURCE_FILES = [
  'scalars.yaml',
  'entities.yaml',
  'events.yaml',
  'commands.yaml',
  'errors.yaml',
  'actors.yaml',
  'views.yaml',
  'api.yaml',
  'stories.yaml',
  'rules.yaml',
  'tests.yaml',
  'translations.yaml',
  'customer_screens.yaml',
  'observability.yaml',
  'architecture/c4-l2.yaml',
  'architecture/c4-l3.yaml',
] as const;

export type SourceFile = (typeof SOURCE_FILES)[number];

/** One entry of an actor's inbox. */
export interface ReceiveEntry {
  /** The command or event received (a `$ref` into commands.yaml / events.yaml). */
  message: RefNode;
  /** Events appended in response (`$ref`s into events.yaml). May be empty for a pure reaction. */
  emits: RefNode[];
  /** Domain errors this message may raise on rejection (`$ref`s into errors.yaml). */
  throws: RefNode[];
  /** Optional note when `emits` is empty or the reaction is non-obvious. */
  effect?: string;
}

export type ActorKind = 'aggregate' | 'process-manager';

export interface Actor {
  name: string;
  type: ActorKind;
  description?: string;
  receives: ReceiveEntry[];
}

/** A column of a `View_*` read table. */
export interface ViewColumn {
  name: string;
  /**
   * SQL primitive or a scalars.yaml type name (validated). May be DERIVED from `from` (the source
   * event property's type) when not declared explicitly; empty string if neither is available (a hole).
   */
  type: string;
  /** True when `type` was derived from a `from` event-property rather than declared on the column. */
  typeDerived?: boolean;
  /**
   * Lineage: the event (or event property) `$ref`s that populate this column, e.g.
   * `events.yaml#/RestaurantRegistered/properties/slug` (property) or `events.yaml#/OrderPlaced`
   * (whole event, for derived/status columns). Each event must be in the view's `fedBy`.
   */
  from?: string[];
  pk?: boolean;
  unique?: boolean;
  index?: boolean;
  nullable?: boolean;
  /** Foreign key into another view, as `"View_Name.column"` — declares the read navigation graph. */
  fk?: string;
  note?: string;
}

/** A read model (`View_*` projection table). */
export interface View {
  name: string;
  /** Primary source aggregate (an aggregate in actors.yaml). Empty for a `reference` view. */
  aggregate: string;
  /** Release slice: 'V0' | 'V1'. */
  slice: string;
  /**
   * Static REFERENCE/seed read model (`source: reference`): not fed by events, has no aggregate, and is
   * seeded at deploy time (e.g. phone country/dialing codes). Exempt from event-lineage validation; may
   * back a query's `@reads`.
   */
  reference?: boolean;
  /**
   * Internal read model: consumed by command handlers / auth resolution rather than a GraphQL query
   * (e.g. uniqueness & idempotency lookups). Not expected to be referenced by `@reads`.
   */
  internal?: boolean;
  /** Events consumed by the projection (`$ref`s into events.yaml). */
  fedBy: RefNode[];
  /** Business filters scoping the rows. */
  filters: string[];
  /** Derivation rules / invariants. */
  rules: string[];
  /** Secondary indexes (beyond the PK), each a list of column names. */
  indexes: string[][];
  columns: ViewColumn[];
  note?: string;
}

/** An argument of a query, or a property of an output type / mutation payload. */
export interface ApiField {
  name: string;
  /** Scalar/entity/type name (when `ref`) or an inline primitive (string|boolean|integer). */
  type: string;
  /** True when declared via `$ref` (into scalars/entities), false for an inline primitive type. */
  ref: boolean;
  required?: boolean;
  nullable?: boolean;
  /** JSON-Schema-style list (`array: true`). */
  array?: boolean;
  /** Inline-primitive format (e.g. `date-time` → GraphQL `DateTime`). */
  format?: string;
}

/**
 * A registered GraphQL output type (the resolver registry in api.yaml `types`). Its shape is declared
 * INLINE via `properties` (decoupled from entities.yaml — the read/API shape, not the write shape);
 * `reads` binds it to the View_* read model(s) its resolver loads from (the source of truth for the
 * shape; empty for boundary types returned via a mutation payload).
 */
export interface ApiType {
  name: string;
  description?: string;
  /** views.yaml read models backing this type (→ @reads on the resolver; queries inherit it). */
  reads: string[];
  /** Inline GraphQL output fields (scalars/value-objects by `$ref`, or inline primitives). */
  properties: ApiField[];
}

export interface ApiQuery {
  name: string;
  description?: string;
  args: ApiField[];
  /** Output type name (entities.yaml type or an api `types` projection). */
  returnsType: string;
  returnsList: boolean;
  returnsNullable: boolean;
  /** views.yaml read models this query serves (→ @reads). */
  reads: string[];
  /** scalars.yaml#/UserType values allowed (→ @auth/@public). */
  roles: string[];
  slice: string;
}

export interface ApiMutation {
  name: string;
  description?: string;
  /** commands.yaml command dispatched (→ @command); input type derived from its payload. */
  command: string;
  roles: string[];
  slice: string;
  /** Minimal extra payload fields; `correlationId` is added by the generator. */
  payload: ApiField[];
}

/** The GraphQL API surface, parsed from api.yaml. */
export interface Api {
  types: ApiType[];
  queries: ApiQuery[];
  mutations: ApiMutation[];
  /** Subscriptions: same shape as queries (args + return type), but streamed — no `@reads`. */
  subscriptions: ApiQuery[];
}

/** One step of a story activity: references an api op (`opKind`+`op`) OR is a note-only step. */
export interface StoryStep {
  name: string;
  /** 'query' | 'mutation' when the step references an api operation. */
  opKind?: 'query' | 'mutation';
  /** The api.yaml operation name the step realizes. */
  op?: string;
  note?: string;
}

export interface StoryActivity {
  name: string;
  description?: string;
  steps: StoryStep[];
}

/** A persona from the story map (stories.yaml), mapped to a UserType via `role`. */
export interface Persona {
  name: string;
  description?: string;
  /** scalars.yaml#/UserType value this persona acts as. */
  role: string;
  /** Expected language/culture (scalars.yaml#/Locale), e.g. 'fr-FR'. */
  locale?: string;
  activities: StoryActivity[];
}

export interface Model {
  /** Raw definition maps, keyed by source file then by definition name. */
  defs: Record<SourceFile, Record<string, SchemaNode>>;
  /** File-level metadata kept aside (version/description), keyed by source file. */
  meta: Record<SourceFile, { version?: number; description?: string }>;
  /** Parsed actor catalog (the typed view over `actors.yaml`). */
  actors: Actor[];
  /** Parsed read models (the typed view over `views.yaml`). */
  views: View[];
  /** Events deliberately not projected into any view (transient/saga-internal; views.yaml). */
  nonProjectedEvents: string[];
  /** Parsed API surface (the typed view over `api.yaml`). */
  api: Api;
  /** Parsed story-map personas (the typed view over `stories.yaml`). */
  personas: Persona[];
}

/** Helper: every definition name in a file. */
export function names(model: Model, file: SourceFile): string[] {
  return Object.keys(model.defs[file]);
}
