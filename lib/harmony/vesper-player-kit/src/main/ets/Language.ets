/** Comparison keys preserve suffixes and never replace the native catalog tag. */
export function languageComparisonKey(tag: string): string {
  const parts = tag.toLowerCase().split('-');
  switch (parts[0]) {
    case 'eng': parts[0] = 'en'; break;
    case 'jpn': parts[0] = 'ja'; break;
    case 'zho': case 'chi': parts[0] = 'zh'; break;
    case 'fra': case 'fre': parts[0] = 'fr'; break;
    case 'deu': case 'ger': parts[0] = 'de'; break;
    case 'spa': parts[0] = 'es'; break;
    case 'kor': parts[0] = 'ko'; break;
    case 'rus': parts[0] = 'ru'; break;
    case 'ita': parts[0] = 'it'; break;
    case 'por': parts[0] = 'pt'; break;
    case 'ara': parts[0] = 'ar'; break;
  }
  return parts.join('-');
}

interface LanguageParts { language: string; script?: string; region?: string; }
function languageParts(tag: string): LanguageParts | undefined {
  const parts = languageComparisonKey(tag).split('-');
  if (parts[0].length === 0 || parts[0] === 'und') { return undefined; }
  const value: LanguageParts = { language: parts[0] }; let index = 1;
  if (/^[a-z]{4}$/.test(parts[index] ?? '')) { value.script = parts[index++]; }
  if (/^([a-z]{2}|[0-9]{3})$/.test(parts[index] ?? '')) { value.region = parts[index++]; }
  // Variants, extensions and extlangs require an exact key rather than a lossy fallback.
  return index === parts.length ? value : undefined;
}

export function languageFallbackMatches(nativeTag: string, preferredTag: string): boolean {
  const native = languageParts(nativeTag); const preferred = languageParts(preferredTag);
  return native !== undefined && preferred !== undefined && native.language === preferred.language &&
    (native.script === undefined || preferred.script === undefined || native.script === preferred.script) &&
    (native.region === undefined || preferred.region === undefined || native.region === preferred.region);
}
