// *.rs and *.html files imported as text (wrangler.jsonc "rules").
declare module "*.rs" {
  const text: string;
  export default text;
}

// The landing page, imported as text (wrangler.jsonc "rules"): the sources
// its Run buttons send are allowed by their hash.
declare module "*.html" {
  const text: string;
  export default text;
}
