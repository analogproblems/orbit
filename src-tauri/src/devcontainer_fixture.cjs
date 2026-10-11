// Disposable stand-in for `docker` and `devcontainer`. It never starts a
// container: a "container" is a folder under the state folder, and commands
// run in it with the host's shell. Usage: node devcontainer_fixture.cjs
// <state folder> <docker|devcontainer> <arguments...>. Each call is logged
// as a JSON line in <state>/log. Files in the state folder change behaviour:
// `no-daemon` fails `docker version`, `build-fails` fails builds.
const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const [state, tool, ...args] = process.argv.slice(2);
fs.mkdirSync(state, { recursive: true });
fs.appendFileSync(
  path.join(state, "log"),
  JSON.stringify([tool, ...args]) + "\n",
);
const has = (name) => fs.existsSync(path.join(state, name));
const containers = path.join(state, "containers");
const images = path.join(state, "images");
const imageFile = (image) => path.join(images, encodeURIComponent(image));
const root = (name) => path.join(containers, name, "fs");
// Container paths are kept under the container's folder.
const inside = (name, p) => path.join(root(name), p);
const fail = (message, code = 1) => {
  process.stderr.write(message + "\n");
  process.exit(code);
};
const after = (list, flag) => list[list.indexOf(flag) + 1];

if (tool === "devcontainer") {
  if (args[0] === "--version") {
    console.log("0.80.1");
  } else if (args[0] === "build") {
    if (has("build-fails")) fail("ERR: the recipe's build failed");
    const folder = after(args, "--workspace-folder");
    const recipe = fs.readFileSync(
      path.join(folder, ".devcontainer", "devcontainer.json"),
      "utf8",
    );
    fs.mkdirSync(images, { recursive: true });
    fs.writeFileSync(imageFile(after(args, "--image-name")), recipe);
    console.log('{"outcome":"success"}');
  } else {
    fail("unknown devcontainer command");
  }
  process.exit(0);
}

const [command, ...rest] = args;
switch (command) {
  case "version":
    if (has("no-daemon")) fail("Cannot connect to the Docker daemon");
    console.log("29.0.0");
    break;
  case "image": {
    const image = rest[rest.length - 1];
    if (!fs.existsSync(imageFile(image))) fail(`No such image: ${image}`);
    if (after(rest, "--format").includes("devcontainer.metadata")) {
      console.log(JSON.stringify([{ id: "base" }, { remoteUser: "dev" }]));
    } else {
      console.log("sha256:fake");
    }
    break;
  }
  case "run": {
    const name = after(rest, "--name");
    fs.mkdirSync(root(name), { recursive: true });
    fs.writeFileSync(
      path.join(containers, name, "run.json"),
      JSON.stringify(rest),
    );
    console.log("container-id");
    break;
  }
  case "cp": {
    const [source, target] = rest;
    const [name, folder] = target.split(":");
    fs.cpSync(source, inside(name, folder), { recursive: true });
    break;
  }
  case "exec": {
    const workdir = rest.includes("--workdir") ? after(rest, "--workdir") : "/";
    // Skips the options, each with its value.
    let i = 0;
    while (rest[i].startsWith("--")) i += 2;
    const [name, program, ...argv] = rest.slice(i);
    if (!fs.existsSync(root(name))) fail(`No such container: ${name}`);
    if (program === "mkdir") {
      fs.mkdirSync(inside(name, argv[argv.length - 1]), { recursive: true });
    } else if (program === "rm") {
      fs.rmSync(inside(name, argv[argv.length - 1]), { force: true });
    } else if (program === "chown") {
      // Nothing to change in a folder.
    } else if (program === "/bin/sh") {
      const result = spawnSync(argv[1], {
        cwd: inside(name, workdir),
        shell: true,
        stdio: "inherit",
      });
      process.exit(result.status ?? 1);
    } else {
      fail(`unknown program ${program}`);
    }
    break;
  }
  case "rm": {
    fs.rmSync(path.join(containers, rest[rest.length - 1]), {
      recursive: true,
      force: true,
    });
    break;
  }
  case "ps":
    if (fs.existsSync(containers)) {
      for (const name of fs.readdirSync(containers)) console.log(name);
    }
    break;
  default:
    fail(`unknown docker command ${command}`);
}
