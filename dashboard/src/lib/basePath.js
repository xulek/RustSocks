const detectBasePath = () => {
  if (typeof window === 'undefined') {
    return '';
  }

  const scripts = Array.from(document.getElementsByTagName('script'));
  for (const script of scripts) {
    if (!script.src) continue;

    try {
      const url = new URL(script.src, window.location.origin);
      const markerIndex = url.pathname.indexOf('/assets/');
      if (markerIndex !== -1) {
        const candidate = url.pathname.slice(0, markerIndex);
        if (!candidate || candidate === '/') {
          return '';
        }
        return candidate.replace(/\/+$/, '');
      }
    } catch (err) {
      // Ignore malformed URLs or relative paths that cannot be resolved.
    }
  }

  // Fallback: attempt to infer from current location (useful during development).
  const path = window.location.pathname;
  if (!path || path === '/' || path === '') {
    return '';
  }

  const segments = path.split('/').filter(Boolean);
  if (segments.length === 0) {
    return '';
  }

  // Assume first segment corresponds to the mounted base path.
  return `/${segments[0]}`;
};

const resolveBasePath = () => {
  if (typeof window === 'undefined') {
    return '';
  }

  const existing = window.__RUSTSOCKS_BASE_PATH__;
  if (typeof existing === 'string') {
    if (!existing || existing === '/') {
      return '';
    }
    return existing.replace(/\/+$/, '');
  }

  const detected = detectBasePath();
  window.__RUSTSOCKS_BASE_PATH__ = detected;
  return detected;
};

export const BASE_PATH = resolveBasePath();
export const ROUTER_BASENAME = BASE_PATH || '/';

export const withBasePath = (path) => {
  const normalizedPath = path.startsWith('/') ? path : `/${path}`;
  if (!BASE_PATH) {
    return normalizedPath;
  }
  return `${BASE_PATH}${normalizedPath}`;
};

export const getApiUrl = (path) => withBasePath(path);
