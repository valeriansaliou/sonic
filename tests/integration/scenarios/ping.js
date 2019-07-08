// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Nikita Vilunov <nikitaoryol@gmail.com>
// License: Mozilla Public License v2.0 (MPL v2.0)

async function run(search) {
  // Perform a ping
  await search.ping();
}

require("../runner/runner.js")(
  "Ping", run
);
