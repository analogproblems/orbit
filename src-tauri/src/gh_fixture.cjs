// A fake `gh api` for tests, holding a repository's issues and comments in
// <dir>/github.json. Usage: node gh_fixture.cjs <dir> api --hostname github.com
// --method <METHOD> <endpoint> [-f key=value ...] [--input -]
// GraphQL answers with <dir>/answer.json, or <dir>/commit-<oid>.json for a
// query about one commit. Compare answers from <dir>/compare.json, keyed by
// "base...head", or 404. While <dir>/down exists every call
// fails. While <dir>/hold exists a GET waits, after writing <dir>/held. Each
// call is logged to <dir>/calls as "METHOD endpoint".
const fs = require("node:fs");
const path = require("node:path");

const [dir, , ...args] = process.argv.slice(2);
const file = path.join(dir, "github.json");
const load = () =>
  fs.existsSync(file)
    ? JSON.parse(fs.readFileSync(file, "utf8"))
    : { next: 100, issues: [], comments: [] };
const save = (db) => fs.writeFileSync(file, JSON.stringify(db, null, 2));

const method = args[args.indexOf("--method") + 1];
const endpoint = args[args.indexOf("--method") + 2];
const fields = {};
args.forEach((a, i) => {
  if (a === "-f") {
    const [k, ...v] = args[i + 1].split("=");
    fields[k] = v.join("=");
  }
});
const input = args.includes("--input")
  ? JSON.parse(fs.readFileSync(0, "utf8"))
  : null;
fs.appendFileSync(path.join(dir, "calls"), `${method} ${endpoint}\n`);
if (fs.existsSync(path.join(dir, "down"))) {
  process.stderr.write("gh: Bad Gateway (HTTP 502)\n");
  process.exit(1);
}
const answer = (v) => process.stdout.write(JSON.stringify(v));
const missing = () => {
  process.stderr.write("gh: Not Found (HTTP 404)\n");
  process.exit(1);
};
const me = { login: "me" };
const url = (n, id) =>
  `https://github.com/a/b/issues/${n}${id ? `#issuecomment-${id}` : ""}`;
// Comments and issues are made a second apart, by ID.
const made = (id) =>
  new Date(Date.UTC(2026, 9, 29, 12) + id * 1000)
    .toISOString()
    .replace(".000Z", "Z");
const page = (list) => {
  const per = Number(fields.per_page || 30);
  const p = Number(fields.page || 1);
  return list.slice((p - 1) * per, p * per);
};

if (endpoint === "graphql") {
  const commit = path.join(dir, `commit-${fields.oid}.json`);
  const file = fields.oid && fs.existsSync(commit) ? commit : "answer.json";
  process.stdout.write(fs.readFileSync(path.resolve(dir, file), "utf8"));
  process.exit(0);
}
if (method === "GET" && fs.existsSync(path.join(dir, "hold"))) {
  fs.writeFileSync(path.join(dir, "held"), "");
  const nap = new Int32Array(new SharedArrayBuffer(4));
  while (fs.existsSync(path.join(dir, "hold"))) Atomics.wait(nap, 0, 0, 20);
}
const db = load();
let m;
if ((m = endpoint.match(/^repos\/a\/b\/compare\/(.+)$/))) {
  const compare = path.join(dir, "compare.json");
  const known = fs.existsSync(compare)
    ? JSON.parse(fs.readFileSync(compare, "utf8"))
    : {};
  if (!known[m[1]]) missing();
  answer(known[m[1]]);
} else if ((m = endpoint.match(/^repos\/a\/b\/issues\/(\d+)\/comments$/))) {
  const n = Number(m[1]);
  if (method === "GET")
    answer(
      page(
        db.comments.filter(
          (c) =>
            c.issue === n && (!fields.since || c.created_at >= fields.since),
        ),
      ),
    );
  else {
    const c = {
      id: db.next++,
      issue: n,
      user: me,
      body: input.body,
      html_url: "",
    };
    c.created_at = made(c.id);
    c.html_url = url(n, c.id);
    db.comments.push(c);
    save(db);
    answer(c);
  }
} else if ((m = endpoint.match(/^repos\/a\/b\/issues\/comments\/(\d+)$/))) {
  const c = db.comments.find((c) => c.id === Number(m[1]));
  if (!c) missing();
  c.body = input.body;
  save(db);
  answer(c);
} else if (endpoint === "repos/a/b/issues") {
  if (method === "GET") {
    const mine = db.issues
      .filter((i) => i.user.login === fields.creator)
      .sort((a, b) => b.number - a.number);
    answer(page(mine));
  } else {
    const i = {
      id: db.next++,
      number: db.next++,
      user: me,
      title: input.title,
      body: input.body,
      labels: input.labels.map((name) => ({ name })),
      state: "open",
      state_reason: null,
      html_url: "",
    };
    i.created_at = made(i.number);
    i.html_url = url(i.number);
    db.issues.push(i);
    save(db);
    answer(i);
  }
} else if ((m = endpoint.match(/^repos\/a\/b\/issues\/(\d+)$/))) {
  const i = db.issues.find((i) => i.number === Number(m[1]));
  if (!i) missing();
  Object.assign(i, input);
  save(db);
  answer(i);
} else {
  answer([]);
}
