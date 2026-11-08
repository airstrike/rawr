// Every page is served over TLS. A plain-http request moves to the same URL
// on https, keeping its path and query; a GET or HEAD gets a 301, anything
// else a 308 so a POST stays a POST.
export function secured(request: Request): Response | null {
  const url = new URL(request.url);
  if (url.protocol !== "http:") return null;
  url.protocol = "https:";
  const status = request.method === "GET" || request.method === "HEAD" ? 301 : 308;
  return Response.redirect(url.toString(), status);
}
