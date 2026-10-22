// *.rs files imported as text (wrangler.jsonc "rules").
declare module "*.rs" {
  const text: string;
  export default text;
}
