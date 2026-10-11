// Disposable stand-in for `docker` and `devcontainer`. It never starts a
// container: a "container" is a folder under the state folder, and commands
// run in it with the host's shell. Usage: node devcontainer_fixture.cjs
// <state folder> <docker|devcontainer> <arguments...>. Each call is logged
// as a JSON line in <state>/log. Files in the state folder change behaviour:
// `no-daemon` fails `docker version`, `build-fails` fails builds,
// `slow-start` makes a container appear a second after `docker run`, and
// `stop-fails` fails stopping a command that ran out of time.
// Like Docker's daemon, a started container and the commands run in it keep
// going when this script is stopped; `docker rm` stops them.
const fs = require("node:fs");
const path = require("node:path");
const { spawn, spawnSync } = require("node:child_process");

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
// Stops the commands run in a container, and what they started.
const stopAll = (name) => {
  const pids = path.join(containers, name, "pids");
  if (!fs.existsSync(pids)) return;
  for (const pid of fs.readFileSync(pids, "utf8").split("\n")) {
    try {
      if (pid) process.kill(-Number(pid), "SIGKILL");
    } catch {
      // It finished already.
    }
  }
  fs.writeFileSync(pids, "");
};

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
    if (after(rest, "--format") === "{{json .Config}}") {
      // An image's users come from its recipe: `remoteUser` and
      // `containerUser` go in the Dev Container CLI's metadata, and the
      // fake-only `dockerfileUser` plays a Dockerfile's USER.
      const recipe = JSON.parse(fs.readFileSync(imageFile(image), "utf8"));
      const users = {};
      for (const key of ["remoteUser", "containerUser"]) {
        if (recipe[key]) users[key] = recipe[key];
      }
      const metadata = JSON.stringify([{ id: "base" }, users]);
      console.log(
        JSON.stringify({
          User: recipe.dockerfileUser ?? "",
          Labels: { "devcontainer.metadata": metadata },
        }),
      );
    } else {
      console.log("sha256:fake");
    }
    break;
  }
  case "run": {
    const name = after(rest, "--name");
    const create = `
      const fs = require("node:fs");
      fs.mkdirSync(${JSON.stringify(root(name))}, { recursive: true });
      fs.writeFileSync(
        ${JSON.stringify(path.join(containers, name, "run.json"))},
        ${JSON.stringify(JSON.stringify(rest))},
      );`;
    // The "daemon" creates the container, even if this script is stopped.
    const delay = has("slow-start") ? 1000 : 0;
    spawnSync(
      process.execPath,
      ["-e", `setTimeout(() => {${create}}, ${delay})`],
      {
        stdio: "inherit",
      },
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
    } else if (program === "/bin/sh" && argv[1].startsWith("kill -KILL -1")) {
      // Kills everything started in the container, never on this computer;
      // `stop-fails` plays a container with no room for another process.
      if (has("stop-fails"))
        fail(
          "OCI runtime exec failed: unable to start container process: resource temporarily unavailable",
          126,
        );
      stopAll(name);
      console.log("stopped");
    } else if (program === "/bin/sh") {
      // In its own process group, which `docker rm` and `kill -KILL -1`
      // stop.
      const child = spawn(argv[1], {
        cwd: inside(name, workdir),
        shell: true,
        stdio: "inherit",
        detached: true,
      });
      fs.appendFileSync(path.join(containers, name, "pids"), `${child.pid}\n`);
      child.on("exit", (code) => process.exit(code ?? 1));
    } else {
      fail(`unknown program ${program}`);
    }
    break;
  }
  case "rm": {
    const container = path.join(containers, rest[rest.length - 1]);
    stopAll(rest[rest.length - 1]);
    if (!fs.existsSync(container)) fail(`No such container: ${rest.at(-1)}`);
    fs.rmSync(container, { recursive: true, force: true });
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
