import { computed, type Signal } from "@preact/signals"

/** Creates a writable projected signal from explicit read and write operations. */
export function lens<T>(get: () => T, set: (value: T) => void): Signal<T> {
  const projected = computed(get) as Signal<T>
  const descriptor = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(projected), "value")!

  Object.defineProperty(projected, "value", {
    configurable: true,
    enumerable: descriptor.enumerable,
    get: descriptor.get!.bind(projected),
    set,
  })

  return projected
}
