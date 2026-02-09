import fs from "node:fs";
import path from "node:path";
import http from "node:http";

function readArg(name, fallback) {
  const argv = process.argv.slice(2);
  const index = argv.indexOf(name);
  if (index < 0 || index + 1 >= argv.length) {
    return fallback;
  }
  return argv[index + 1];
}

function contentTypeFor(filePath) {
  const ext = path.extname(filePath).toLowerCase();
  switch (ext) {
    case ".html":
      return "text/html; charset=utf-8";
    case ".css":
      return "text/css; charset=utf-8";
    case ".js":
      return "text/javascript; charset=utf-8";
    case ".json":
      return "application/json; charset=utf-8";
    case ".png":
      return "image/png";
    case ".jpg":
    case ".jpeg":
      return "image/jpeg";
    case ".svg":
      return "image/svg+xml";
    case ".ico":
      return "image/x-icon";
    default:
      return "application/octet-stream";
  }
}

function safeResolve(rootDir, requestPathname) {
  const decoded = decodeURIComponent(requestPathname || "/");
  const pathname = decoded === "/" ? "/index.html" : decoded;
  const normalized = path.normalize(pathname).replace(/^(\.\.(\/|\\|$))+/, "");
  const resolved = path.resolve(rootDir, `.${normalized}`);
  if (resolved === rootDir || resolved.startsWith(`${rootDir}${path.sep}`)) {
    return resolved;
  }
  return null;
}

function respondNotFound(response) {
  response.statusCode = 404;
  response.setHeader("Content-Type", "text/plain; charset=utf-8");
  response.end("Not Found");
}

const rootDir = path.resolve(process.cwd(), readArg("--root", "./dist"));
const portRaw = readArg("--port", "4173");
const port = Number.parseInt(portRaw, 10);

if (!Number.isFinite(port) || port <= 0) {
  console.error(`Invalid --port value: ${portRaw}`);
  process.exit(1);
}

if (!fs.existsSync(rootDir)) {
  console.error(`Static root not found: ${rootDir}`);
  process.exit(1);
}

const server = http.createServer((request, response) => {
  if (request.method !== "GET" && request.method !== "HEAD") {
    response.statusCode = 405;
    response.setHeader("Content-Type", "text/plain; charset=utf-8");
    response.end("Method Not Allowed");
    return;
  }

  const url = new URL(request.url || "/", `http://${request.headers.host || "127.0.0.1"}`);
  const resolved = safeResolve(rootDir, url.pathname);
  if (!resolved) {
    response.statusCode = 403;
    response.setHeader("Content-Type", "text/plain; charset=utf-8");
    response.end("Forbidden");
    return;
  }

  let target = resolved;
  if (fs.existsSync(target) && fs.statSync(target).isDirectory()) {
    target = path.join(target, "index.html");
  }

  if (!fs.existsSync(target) || !fs.statSync(target).isFile()) {
    respondNotFound(response);
    return;
  }

  response.statusCode = 200;
  response.setHeader("Content-Type", contentTypeFor(target));
  if (request.method === "HEAD") {
    response.end();
    return;
  }

  const stream = fs.createReadStream(target);
  stream.on("error", () => respondNotFound(response));
  stream.pipe(response);
});

server.listen(port, "127.0.0.1", () => {
  console.log(`Static server started on http://127.0.0.1:${port} (root: ${rootDir})`);
});
