export const APP_NAME = 'Rivulet'
export const APP_SCHEME = 'rivulet'
export const STORAGE_PREFIX = 'rivulet.'
export const BUNDLE_ID = 'io.github.rivulet.Rivulet'
export const APP_HOST = 'rivulet.localhost'

/** Key helper for localStorage with the app's brand prefix. */
export function key(name: string): string {
  return `${STORAGE_PREFIX}${name}`
}
