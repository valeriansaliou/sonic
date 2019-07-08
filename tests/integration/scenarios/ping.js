// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Nikita Vilunov <nikitaoryol@gmail.com>
// License: Mozilla Public License v2.0 (MPL v2.0)

async function run(search) {
    await search.ping();
}

require("../runner.js")("Ping", run);
