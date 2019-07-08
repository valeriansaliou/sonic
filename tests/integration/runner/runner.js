// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Nikita Vilunov <nikitaoryol@gmail.com>
// License: Mozilla Public License v2.0 (MPL v2.0)

const SonicChannel = require("sonic-channel");

function connect(chan, name) {
    return new Promise((resolve, reject) => {
        chan.connect({
            connected() {
                console.info(`=== Sonic Channel succeeded to connect to host (${name}) ===`);
                resolve(chan);
            },
            disconnected() {
                console.error(`=== Sonic Channel is now disconnected (${name}) ===`);
            },
            timeout() {
                console.error(`=== Sonic Channel connection timed out (${name}) ===`);
            },
            retrying() {
                console.error(`=== Trying to reconnect to Sonic Channel (${name}) ===`);
            },
            error(error) {
                console.error(`=== Sonic Channel failed to connect to host (${name}) ===`, error);
                reject(error);
            }
        });
    });
}

async function main(scenario) {
    let params = {
        host: "localhost", port: 1491, auth: "SecretPassword"
    };
    let search = new SonicChannel.Search(params);
    let ingest = new SonicChannel.Ingest(params);
    await Promise.all([connect(search, "search"), connect(ingest, "ingest")]);
    await scenario(search, ingest);
    await Promise.all([search.close(), ingest.close()]);
}

function wrapper(name, scenario, timeout) {
    console.log(`=== Running test scenario ${name} ===`)
    timeout = timeout || 1000;
    let timer = new Promise((_, reject) => {
        setTimeout(() => { reject("Timeout reached"); }, timeout)
    });
    let start = process.hrtime();

    Promise.race([main(scenario), timer]).then(
        () => {
            let end = process.hrtime(start);
            console.log(`=== Test scenario ${name} succedeed, execution time: ${end[0] + end[1] / 1e9} s ===`)
        },
        (error)  => {
            let end = process.hrtime(start);
            console.error(`=== Test scenario ${name} failed, execution time: ${end[0] + end[1] / 1e9} s ===`, error)
            process.exit(-1);
        }
    );
}

module.exports = wrapper;
