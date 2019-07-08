const expectedDocuments = {
    "conversation:1": "Batch normalization is a technique for improving the speed, performance, and stability of artificial neural networks",
    "conversation:2": "This scratch technique is much like the transform in some ways",
};

const unexpectedDocuments = {
    "conversation:3": "Glissando is a glide from one pitch to another"
}

async function run(search, ingest) {
    for (const key in expectedDocuments) {
        await ingest.push("messages", "default", key, expectedDocuments[key]);
    }
    for (const key in unexpectedDocuments) {
        await ingest.push("messages", "default", key, unexpectedDocuments[key]);
    }

    let res = await search.query("messages", "default", "technique");
    for (const key in expectedDocuments) {
        if(!res.includes(key))
            throw `Expected document ${key} was not found`;
    }
    for (const key in unexpectedDocuments) {
        if(res.includes(key))
            throw `Unexpected document ${key} was returned`;
    }
}

require("../runner.js")("Insert & Search", run);
