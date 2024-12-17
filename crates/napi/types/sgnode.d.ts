import type {
  FieldNames,
  TypesInField,
  TypesMap,
  ExtractField,
  Kinds,
  NodeFieldInfo,
  RootKind,
} from './staticTypes'
import type { NapiConfig } from './config'

export interface Edit {
  /** The start position of the edit */
  startPos: number
  /** The end position of the edit */
  endPos: number
  /** The text to be inserted */
  insertedText: string
}
export interface Pos {
  /** line number starting from 0 */
  line: number
  /** column number starting from 0 */
  column: number
  /** byte offset of the position */
  index: number
}
export interface Range {
  /** starting position of the range */
  start: Pos
  /** ending position of the range */
  end: Pos
}

export declare class SgNode<
  M extends TypesMap = TypesMap,
  out T extends Kinds<M> = Kinds<M>,
> {
  range(): Range
  isLeaf(): boolean
  isNamed(): boolean
  isNamedLeaf(): boolean
  /** Returns the string name of the node kind */
  kind(): T
  readonly kindToRefine: T
  /** Check if the node is the same kind as the given `kind` string */
  is<K extends Kinds<M>>(kind: K): this is SgNode<M, K>
  text(): string
  matches(m: string): boolean
  inside(m: string): boolean
  has(m: string): boolean
  precedes(m: string): boolean
  follows(m: string): boolean
  getMatch<K extends Kinds<M>>(m: string): RefineNode<M, K> | null
  getMultipleMatches(m: string): Array<SgNode<M>>
  getTransformed(m: string): string | null
  /** Returns the node's SgRoot */
  getRoot(): SgRoot<M>
  children(): Array<SgNode<M>>
  /** Returns the node's id */
  id(): number
  find<K extends Kinds<M>>(
    matcher: string | number | NapiConfig<M>,
  ): RefineNode<M, K> | null
  findAll<K extends Kinds<M>>(
    matcher: string | number | NapiConfig<M>,
  ): Array<RefineNode<M, K>>
  /** Finds the first child node in the `field` */
  field<F extends FieldNames<M[T]>>(name: F): FieldNode<M, T, F>
  /** Finds all the children nodes in the `field` */
  fieldChildren<F extends FieldNames<M[T]>>(
    name: F,
  ): Exclude<FieldNode<M, T, F>, null>[]
  parent<K extends Kinds<M>>(): RefineNode<M, K> | null
  child<K extends Kinds<M>>(nth: number): RefineNode<M, K> | null
  ancestors(): Array<SgNode<M>>
  next<K extends Kinds<M>>(): RefineNode<M, K> | null
  nextAll(): Array<SgNode<M>>
  prev<K extends Kinds<M>>(): RefineNode<M, K> | null
  prevAll(): Array<SgNode<M>>
  replace(text: string): Edit
  commitEdits(edits: Array<Edit>): string
}
/** Represents the parsed tree of code. */
export declare class SgRoot<M extends TypesMap = TypesMap> {
  /** Returns the root SgNode of the ast-grep instance. */
  root(): SgNode<M, RootKind<M>>
  /**
   * Returns the path of the file if it is discovered by ast-grep's `findInFiles`.
   * Returns `"anonymous"` if the instance is created by `lang.parse(source)`.
   */
  filename(): string
}

/**
 * if K contains string, return general SgNode. Otherwise,
 * if K is a literal union, return a union of SgNode of each kind.
 */
type RefineNode<M extends TypesMap, K> = string extends K
  ? SgNode<M>
  : K extends Kinds<M>
    ? SgNode<M, K>
    : never

/**
 * return the SgNode of the field in the node.
 */
// F extends string is used to prevent noisy TS hover info
type FieldNode<
  M extends TypesMap,
  K extends Kinds<M>,
  F extends FieldNames<M[K]>,
> = F extends string ? FieldNodeImpl<M, ExtractField<M[K], F>> : never

type FieldNodeImpl<M extends TypesMap, I extends NodeFieldInfo> = I extends {
  required: true
}
  ? RefineNode<M, TypesInField<M, I>>
  : RefineNode<M, TypesInField<M, I>> | null
