const express = require("express");
const fs = require("fs");
const path = require("path");

const app = express();

app.head("*", (req, res) => {
    const file = path.join(__dirname, "resource", req.path);
    const { size } = fs.statSync(file);
    res.setHeader("Content-Length", size);
    res.send();
});

app.get("*", (req, res) => {
    const filePath = path.join(__dirname, "resource", req.path);
    const stat = fs.statSync(filePath);
    const fileSize = stat.size;
    const range = req.headers.range;

    if (range) {
        const parts = range.replace(/bytes=/, "").split("-");
        const start = parseInt(parts[0], 10);
        const end = parts[1] ? parseInt(parts[1], 10) : fileSize - 1;

        const chunkSize = end - start + 1;
        const file = fs.createReadStream(filePath, {start, end});
        const headers = {
            "Content-Range": `bytes ${start}-${end}/${fileSize}`,
            "Accept-Ranges": "bytes",
            "Content-Length": chunkSize,
            "Content-Type": "application/octet-stream",
        };

        res.writeHead(206, headers);
        file.pipe(res);
    } else {
        const headers = {
            "Content-Length": fileSize,
            "Content-Type": "application/octet-stream",
        };
        res.writeHead(200, headers);
        fs.createReadStream(filePath).pipe(res);
    }
});

const PORT = 23333;
app.listen(PORT, () => {
    console.log(`Server started on port ${PORT}`);
});
