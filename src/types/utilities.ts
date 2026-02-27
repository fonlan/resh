export type Nullable<T> = T | null
export type Optional<T> = T | undefined
export type Async<T> = Promise<T>

export type DeepPartial<T> = {
  [P in keyof T]?: T[P] extends object ? DeepPartial<T[P]> : T[P]
}

export type DeepReadonly<T> = {
  readonly [P in keyof T]: T[P] extends object ? DeepReadonly<T[P]> : T[P]
}

export type PickByType<T, U> = {
  [K in keyof T as T[K] extends U ? K : never]: T[K]
}

export type OmitByType<T, U> = {
  [K in keyof T as T[K] extends U ? never : K]: T[K]
}

export type RequiredKeys<T> = {
  [K in keyof T]-?: {} extends Pick<T, K> ? never : K
}[keyof T]

export type OptionalKeys<T> = {
  [K in keyof T]-?: {} extends Pick<T, K> ? K : never
}[keyof T]

export type MakeRequired<T, K extends keyof T> = Omit<T, K> &
  Required<Pick<T, K>>

export type MakeOptional<T, K extends keyof T> = Omit<T, K> &
  Partial<Pick<T, K>>

export type UnionToIntersection<U> = (
  U extends any ? (k: U) => void : never
) extends (k: infer I) => void
  ? I
  : never

export type DistributiveOmit<T, K extends PropertyKey> = T extends any
  ? Omit<T, K>
  : never

export type FunctionParams<T extends (...args: any) => any> = T extends (
  ...args: infer P
) => any
  ? P
  : never

export type FunctionReturn<T extends (...args: any) => any> = T extends (
  ...args: any
) => infer R
  ? R
  : never

export type AnyFunction = (...args: any[]) => any

export type ConstRecord<K extends string | number | symbol, V> = {
  [key in K]: V
}

export type NonEmptyArray<T> = [T, ...T[]]

export type UniqueArray<T> = T[] & { 0: T }

export type TypedKeyof<T> = keyof T & string

export function isNotNull<T>(value: Nullable<T>): value is T {
  return value !== null
}

export function isNotUndefined<T>(value: Optional<T>): value is T {
  return value !== undefined
}

export function isTruthy<T>(value: T): value is NonNullable<T> {
  return Boolean(value)
}

export function isFalsy<T>(value: T): boolean {
  return !value
}

export function isString(value: unknown): value is string {
  return typeof value === "string"
}

export function isNumber(value: unknown): value is number {
  return typeof value === "number" && !Number.isNaN(value)
}

export function isBoolean(value: unknown): value is boolean {
  return typeof value === "boolean"
}

export function isArray(value: unknown): value is unknown[] {
  return Array.isArray(value)
}

export function isObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !isArray(value)
}

export function isFunction(value: unknown): value is AnyFunction {
  return typeof value === "function"
}

export function isEmptyString(value: unknown): value is "" {
  return value === ""
}

export function isNonEmptyString(value: unknown): value is string {
  return isString(value) && value.length > 0
}

export function isPositiveNumber(value: unknown): value is number {
  return isNumber(value) && value > 0
}

export function isInteger(value: unknown): value is number {
  return isNumber(value) && Number.isInteger(value)
}

export function isInRange(value: number, min: number, max: number): boolean {
  return value >= min && value <= max
}

export function hasProperty<T, K extends string>(
  obj: T,
  key: K,
): obj is T & Record<K, unknown> {
  return key in (obj as object)
}

export function getProperty<T, K extends keyof T>(
  obj: T,
  key: K,
): T[K] | undefined {
  return obj[key]
}

export function pick<T, K extends keyof T>(obj: T, keys: K[]): Pick<T, K> {
  const result = {} as Pick<T, K>
  keys.forEach((key) => {
    if (key in (obj as object)) {
      result[key] = obj[key]
    }
  })
  return result
}

export function omit<T, K extends keyof T>(obj: T, keys: K[]): Omit<T, K> {
  const result = { ...(obj as Record<string, unknown>) }
  keys.forEach((key) => {
    delete result[key as string]
  })
  return result as Omit<T, K>
}

export function omitUndefined<T extends Record<string, unknown>>(
  obj: T,
): Partial<T> {
  return Object.fromEntries(
    Object.entries(obj).filter(([_, value]) => value !== undefined),
  ) as Partial<T>
}

export function omitNull<T extends Record<string, unknown>>(
  obj: T,
): Partial<T> {
  return Object.fromEntries(
    Object.entries(obj).filter(([_, value]) => value !== null),
  ) as Partial<T>
}

export function removeEmpty<T extends Record<string, unknown>>(
  obj: T,
): Partial<T> {
  return Object.fromEntries(
    Object.entries(obj).filter(
      ([_, value]) =>
        value !== undefined &&
        value !== null &&
        (typeof value === "string" ? value.length > 0 : true) &&
        (Array.isArray(value) ? value.length > 0 : true),
    ),
  ) as Partial<T>
}

export function groupBy<T>(array: T[], key: keyof T): Record<string, T[]> {
  return array.reduce(
    (groups, item) => {
      const groupKey = String(item[key])
      if (!groups[groupKey]) {
        groups[groupKey] = []
      }
      groups[groupKey].push(item)
      return groups
    },
    {} as Record<string, T[]>,
  )
}

export function sortBy<T>(
  array: T[],
  key: keyof T,
  order: "asc" | "desc" = "asc",
): T[] {
  return [...array].sort((a, b) => {
    const aVal = a[key]
    const bVal = b[key]
    if (aVal < bVal) return order === "asc" ? -1 : 1
    if (aVal > bVal) return order === "asc" ? 1 : -1
    return 0
  })
}

export function uniqueBy<T>(array: T[], key: keyof T): T[] {
  const seen = new Set()
  return array.filter((item) => {
    const val = item[key]
    if (seen.has(val)) return false
    seen.add(val)
    return true
  })
}

export function debounce<T extends AnyFunction>(
  fn: T,
  delay: number,
): (...args: Parameters<T>) => void {
  let timeoutId: ReturnType<typeof setTimeout>
  return (...args: Parameters<T>) => {
    clearTimeout(timeoutId)
    timeoutId = setTimeout(() => fn(...args), delay)
  }
}

export function throttle<T extends AnyFunction>(
  fn: T,
  limit: number,
): (...args: Parameters<T>) => void {
  let inThrottle = false
  return (...args: Parameters<T>) => {
    if (!inThrottle) {
      fn(...args)
      inThrottle = true
      setTimeout(() => {
        inThrottle = false
      }, limit)
    }
  }
}

export function memoize<T extends AnyFunction>(
  fn: T,
  keyGenerator?: (...args: Parameters<T>) => string,
): T {
  const cache = new Map<string, ReturnType<T>>()
  return ((...args: Parameters<T>) => {
    const key = keyGenerator ? keyGenerator(...args) : JSON.stringify(args)
    if (cache.has(key)) {
      return cache.get(key)
    }
    const result = fn(...args)
    cache.set(key, result)
    return result
  }) as T
}

export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

export function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max)
}

export function randomId(): string {
  return `${Date.now()}-${Math.random().toString(36).substr(2, 9)}`
}

export function generateUuid(): string {
  return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0
    const v = c === "x" ? r : (r & 0x3) | 0x8
    return v.toString(16)
  })
}

export type InferArrayElement<T> = T extends readonly (infer U)[] ? U : never

export type InferPromiseValue<T> = T extends Promise<infer U> ? U : never

export type InferEventTarget<T> = T extends keyof WindowEventMap
  ? WindowEventMap[T]
  : T extends keyof DocumentEventMap
    ? DocumentEventMap[T]
    : never

export type AsyncReturnType<T extends AnyFunction> = T extends (
  ...args: any[]
) => Promise<infer U>
  ? U
  : never
