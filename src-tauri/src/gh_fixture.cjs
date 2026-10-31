// A fake `gh api` for tests, holding a repository's issues and comments in
// <dir>/github.json. Usage: node gh_fixture.cjs <dir> api --hostname github.com
// --method <METHOD> <endpoint> [-f key=value ...] [--input -]
// GraphQL answers with <dir>/answer.json, <dir>/commit-<oid>.json for a
// query about one commit, <dir>/history-<oid>.json for one about a
// commit's history (or no such commit), <dir>/setup-<oid>.json for one
// about a commit's workflow files (or a workflow that runs on pull
// requests), or when a comment or issue was
// made and last edited for a query about one by its node ID. Compare answers from <dir>/compare.json, keyed by
// "base...head", or 404. A job is <dir>/job-<id>.json and its log
// <dir>/log-<id>.txt, or 404; while <dir>/limit exists they answer as past
// GitHub's rate limit. Rerunning a job writes <dir>/rerun-<id>, or fails
// with the text of <dir>/rerun-error while that exists. A run's latest jobs
// are the newest try of each job in it, by name, in pages: a job rerun
// whose next try (<id> + 1000) has no file yet has it queued. While
// <dir>/more-jobs exists, the list claims that many jobs more than it has.
// The account `a` is a person, or an organization while <dir>/org exists.
// The runners of a/b and of the organization a are <dir>/runners-repo.json
// and <dir>/runners-org.json, or none; while <dir>/runners-repo-error or
// <dir>/runners-org-error exists, reading them fails with its text. The
// runs of a/b are the list in <dir>/queued.json, or none, listed newest
// first in pages, those with the status asked for made before `created`
// ("<time") when it is given.
// The cache usage of a/b is <dir>/cache.json, or none, and its artifacts the
// list in <dir>/artifacts.json, or none, in pages; while <dir>/cache-error or
// <dir>/artifacts-error exists, reading them fails with its text.
// Pull request <n> of a/b is <dir>/pull-<n>.json, or 404; while
// <dir>/pull-error exists, reading one fails with its text. Its reviews
// are <dir>/reviews-<n>.json, or none, in pages, and while
// <dir>/reviews-error exists, reading them fails with its text. Branch <b>
// of a/b is <dir>/branch-<b>.json, or unprotected; its protection is
// <dir>/protection-<b>.json, or "Branch not protected", and while
// <dir>/protection-error exists, reading it fails with its text; the
// rulesets on it are <dir>/rules-<b>.json, or none, in pages. A user's
// permission on a/b is <dir>/permission-<login>.json, or write, and while
// <dir>/permission-error exists, reading one fails with its text.
// While <dir>/down exists every call
// fails. While <dir>/hold exists a GET waits, after writing <dir>/held, or
// only a GET of the endpoint it names, when it names one; when it names
// "graphql <owner>/<name>", only reading that repository's CI waits. Each
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
// Comments and issues are made a second apart, by ID, and an edit takes the
// next second too. A list with `since` holds those posted or edited then or
// later, by ID. While <dir>/comments-error exists, listing an issue's
// comments fails with its text; reading one comment still works.
const made = (id) =>
  new Date(Date.UTC(2026, 9, 29, 12) + id * 1000)
    .toISOString()
    .replace(".000Z", "Z");
const page = (list) => {
  const per = Number(fields.per_page || 30);
  const p = Number(fields.page || 1);
  return list.slice((p - 1) * per, p * per);
};

if (endpoint === "graphql" && fields.id) {
  const db = load();
  const node = [...db.comments, ...db.issues].find(
    (n) => n.node_id === fields.id,
  );
  answer({
    data: {
      node: node && {
        createdAt: node.created_at,
        lastEditedAt: node.last_edited_at || null,
      },
    },
  });
  process.exit(0);
}
if (endpoint === "graphql" && (fields.query || "").includes("history(")) {
  fs.appendFileSync(path.join(dir, "calls"), `GET history/${fields.oid}\n`);
  const history = path.join(dir, `history-${fields.oid}.json`);
  if (fs.existsSync(history))
    process.stdout.write(fs.readFileSync(history, "utf8"));
  else answer({ data: { repository: { object: null } } });
  process.exit(0);
}
if (endpoint === "graphql" && fields.workflows) {
  const oid = fields.workflows.split(":")[0];
  fs.appendFileSync(path.join(dir, "calls"), `GET setup/${oid}\n`);
  const setup = path.join(dir, `setup-${oid}.json`);
  if (fs.existsSync(setup))
    process.stdout.write(fs.readFileSync(setup, "utf8"));
  else
    answer({
      data: {
        repository: {
          workflows: {
            entries: [
              {
                name: "ci.yml",
                type: "blob",
                object: { text: "on: pull_request\n", isTruncated: false },
              },
            ],
          },
          root: { entries: [{ name: ".github", type: "tree" }] },
          package: null,
        },
      },
    });
  process.exit(0);
}
const hold = path.join(dir, "hold");
const held = (names) =>
  fs.existsSync(hold) && names.includes(fs.readFileSync(hold, "utf8").trim());
const wait = () => {
  fs.writeFileSync(path.join(dir, "held"), "");
  const nap = new Int32Array(new SharedArrayBuffer(4));
  while (fs.existsSync(hold)) Atomics.wait(nap, 0, 0, 20);
};
if (endpoint === "graphql") {
  if (held([`graphql ${fields.owner}/${fields.name}`])) wait();
  const commit = path.join(dir, `commit-${fields.oid}.json`);
  const file = fields.oid && fs.existsSync(commit) ? commit : "answer.json";
  process.stdout.write(fs.readFileSync(path.resolve(dir, file), "utf8"));
  process.exit(0);
}
if (method === "GET" && held(["", endpoint])) wait();
const db = load();
let m;
if ((m = endpoint.match(/^repos\/a\/b\/compare\/(.+)$/))) {
  const compare = path.join(dir, "compare.json");
  const known = fs.existsSync(compare)
    ? JSON.parse(fs.readFileSync(compare, "utf8"))
    : {};
  if (!known[m[1]]) missing();
  answer(known[m[1]]);
} else if ((m = endpoint.match(/^repos\/a\/b\/actions\/jobs\/(\d+)\/rerun$/))) {
  const refused = path.join(dir, "rerun-error");
  if (fs.existsSync(refused)) {
    process.stderr.write(fs.readFileSync(refused, "utf8"));
    process.exit(1);
  }
  fs.writeFileSync(path.join(dir, `rerun-${m[1]}`), "");
} else if ((m = endpoint.match(/^repos\/a\/b\/actions\/runs\/(\d+)\/jobs$/))) {
  const read = (f) => {
    try {
      return JSON.parse(fs.readFileSync(path.join(dir, f), "utf8"));
    } catch {
      return null;
    }
  };
  const tries = fs
    .readdirSync(dir)
    .filter((f) => /^job-\d+\.json$/.test(f))
    .map(read)
    .filter((j) => j && j.run_id === Number(m[1]));
  for (const j of [...tries]) {
    const next = j.id + 1000;
    if (
      fs.existsSync(path.join(dir, `rerun-${j.id}`)) &&
      !fs.existsSync(path.join(dir, `job-${next}.json`))
    )
      tries.push({
        ...j,
        id: next,
        run_attempt: (j.run_attempt || 1) + 1,
        status: "queued",
        conclusion: null,
      });
  }
  const latest = [];
  for (const j of tries) {
    const at = latest.findIndex((l) => l.name === j.name);
    if (at < 0) latest.push(j);
    else if ((j.run_attempt || 1) > (latest[at].run_attempt || 1))
      latest[at] = j;
  }
  const more = path.join(dir, "more-jobs");
  const extra = fs.existsSync(more) ? Number(fs.readFileSync(more, "utf8")) : 0;
  answer({ total_count: latest.length + extra, jobs: page(latest) });
} else if (
  (m = endpoint.match(/^repos\/a\/b\/actions\/jobs\/(\d+)(\/logs)?$/))
) {
  const job = path.join(dir, m[2] ? `log-${m[1]}.txt` : `job-${m[1]}.json`);
  if (fs.existsSync(path.join(dir, "limit"))) {
    process.stderr.write(
      "gh: API rate limit exceeded for user ID 1. (HTTP 403)\n",
    );
    process.exit(1);
  }
  if (!fs.existsSync(job)) missing();
  process.stdout.write(fs.readFileSync(job, "utf8"));
} else if ((m = endpoint.match(/^repos\/a\/b\/issues\/(\d+)\/comments$/))) {
  const n = Number(m[1]);
  const refused = path.join(dir, "comments-error");
  if (method === "GET" && fs.existsSync(refused)) {
    process.stderr.write(fs.readFileSync(refused, "utf8"));
    process.exit(1);
  }
  if (method === "GET")
    answer(
      page(
        db.comments.filter(
          (c) =>
            c.issue === n &&
            (!fields.since || (c.updated_at || c.created_at) >= fields.since),
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
    c.created_at = c.updated_at = made(c.id);
    c.html_url = url(n, c.id);
    c.node_id = `IC_${c.id}`;
    db.comments.push(c);
    save(db);
    answer(c);
  }
} else if ((m = endpoint.match(/^repos\/a\/b\/issues\/comments\/(\d+)$/))) {
  const c = db.comments.find((c) => c.id === Number(m[1]));
  if (!c) missing();
  if (method !== "GET") {
    c.updated_at = made(db.next++);
    if (input.body !== c.body) c.last_edited_at = c.updated_at;
    c.body = input.body;
    save(db);
  }
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
    i.created_at = i.updated_at = made(i.number);
    i.html_url = url(i.number);
    i.node_id = `I_${i.number}`;
    db.issues.push(i);
    save(db);
    answer(i);
  }
} else if ((m = endpoint.match(/^repos\/a\/b\/issues\/(\d+)$/))) {
  const i = db.issues.find((i) => i.number === Number(m[1]));
  if (!i) missing();
  if (method !== "GET") {
    i.updated_at = made(db.next++);
    if (input.body !== i.body) i.last_edited_at = i.updated_at;
    Object.assign(i, input);
    save(db);
  }
  answer(i);
} else if (endpoint === "users/a") {
  const org = fs.existsSync(path.join(dir, "org"));
  answer({ login: "a", type: org ? "Organization" : "User" });
} else if ((m = endpoint.match(/^(repos\/a\/b|orgs\/a)\/actions\/runners$/))) {
  const which = m[1] === "orgs/a" ? "org" : "repo";
  const refused = path.join(dir, `runners-${which}-error`);
  if (fs.existsSync(refused)) {
    process.stderr.write(fs.readFileSync(refused, "utf8"));
    process.exit(1);
  }
  const list = path.join(dir, `runners-${which}.json`);
  if (fs.existsSync(list)) process.stdout.write(fs.readFileSync(list, "utf8"));
  else answer({ total_count: 0, runners: [] });
} else if (
  (m = endpoint.match(/^repos\/a\/b\/actions\/(cache\/usage|artifacts)$/))
) {
  const which = m[1] === "artifacts" ? "artifacts" : "cache";
  const refused = path.join(dir, `${which}-error`);
  if (fs.existsSync(refused)) {
    process.stderr.write(fs.readFileSync(refused, "utf8"));
    process.exit(1);
  }
  const kept = path.join(dir, `${which}.json`);
  const saved = fs.existsSync(kept)
    ? JSON.parse(fs.readFileSync(kept, "utf8"))
    : null;
  if (which === "cache")
    answer(
      saved || {
        full_name: "a/b",
        active_caches_size_in_bytes: 0,
        active_caches_count: 0,
      },
    );
  else {
    const list = saved || [];
    answer({ total_count: list.length, artifacts: page(list) });
  }
} else if ((m = endpoint.match(/^repos\/a\/b\/pulls\/(\d+)\/reviews$/))) {
  const refused = path.join(dir, "reviews-error");
  if (fs.existsSync(refused)) {
    process.stderr.write(fs.readFileSync(refused, "utf8"));
    process.exit(1);
  }
  const reviews = path.join(dir, `reviews-${m[1]}.json`);
  answer(
    page(
      fs.existsSync(reviews)
        ? JSON.parse(fs.readFileSync(reviews, "utf8"))
        : [],
    ),
  );
} else if ((m = endpoint.match(/^repos\/a\/b\/branches\/(.+)\/protection$/))) {
  const refused = path.join(dir, "protection-error");
  if (fs.existsSync(refused)) {
    process.stderr.write(fs.readFileSync(refused, "utf8"));
    process.exit(1);
  }
  const protection = path.join(
    dir,
    `protection-${decodeURIComponent(m[1])}.json`,
  );
  if (!fs.existsSync(protection)) {
    process.stderr.write("gh: Branch not protected (HTTP 404)\n");
    process.exit(1);
  }
  process.stdout.write(fs.readFileSync(protection, "utf8"));
} else if ((m = endpoint.match(/^repos\/a\/b\/branches\/(.+)$/))) {
  const branch = path.join(dir, `branch-${decodeURIComponent(m[1])}.json`);
  if (fs.existsSync(branch))
    process.stdout.write(fs.readFileSync(branch, "utf8"));
  else answer({ name: decodeURIComponent(m[1]), protected: false });
} else if ((m = endpoint.match(/^repos\/a\/b\/rules\/branches\/(.+)$/))) {
  const rules = path.join(dir, `rules-${decodeURIComponent(m[1])}.json`);
  answer(
    page(
      fs.existsSync(rules) ? JSON.parse(fs.readFileSync(rules, "utf8")) : [],
    ),
  );
} else if (
  (m = endpoint.match(/^repos\/a\/b\/collaborators\/([^/]+)\/permission$/))
) {
  const refused = path.join(dir, "permission-error");
  if (fs.existsSync(refused)) {
    process.stderr.write(fs.readFileSync(refused, "utf8"));
    process.exit(1);
  }
  const permission = path.join(dir, `permission-${m[1]}.json`);
  if (fs.existsSync(permission))
    process.stdout.write(fs.readFileSync(permission, "utf8"));
  else answer({ permission: "write", user: { login: m[1] } });
} else if ((m = endpoint.match(/^repos\/a\/b\/pulls\/(\d+)$/))) {
  const refused = path.join(dir, "pull-error");
  if (fs.existsSync(refused)) {
    process.stderr.write(fs.readFileSync(refused, "utf8"));
    process.exit(1);
  }
  const pull = path.join(dir, `pull-${m[1]}.json`);
  if (!fs.existsSync(pull)) missing();
  process.stdout.write(fs.readFileSync(pull, "utf8"));
} else if (endpoint === "repos/a/b/actions/runs") {
  const queued = path.join(dir, "queued.json");
  const before = Date.parse((fields.created || "").replace(/^</, ""));
  const runs = (
    fs.existsSync(queued) ? JSON.parse(fs.readFileSync(queued, "utf8")) : []
  )
    .filter(
      (r) =>
        (!fields.status || r.status === fields.status) &&
        (Number.isNaN(before) || Date.parse(r.created_at) < before),
    )
    .sort((a, b) => Date.parse(b.created_at) - Date.parse(a.created_at));
  answer({ total_count: runs.length, workflow_runs: page(runs) });
} else {
  answer([]);
}
