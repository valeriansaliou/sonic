async function run(search) {
    await search.ping();
}

require("../runner.js")("Ping", run);
