import Foundation

guard CommandLine.arguments.count == 3 else {
    fputs("Usage: embed-resources.swift RESOURCES_DIRECTORY ICON_PNG\n", stderr)
    exit(2)
}
let directory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let icon = URL(fileURLWithPath: CommandLine.arguments[2])
let image = try Data(contentsOf: icon)
let uri = "data:image/png;base64," + image.base64EncodedString()
guard let files = FileManager.default.enumerator(at: directory, includingPropertiesForKeys: nil) else {
    fputs("Cannot read installer resources.\n", stderr)
    exit(1)
}
for case let file as URL in files where file.pathExtension == "html" {
    let html = try String(contentsOf: file, encoding: .utf8)
    let embedded = html.replacingOccurrences(of: "__APP_ICON__", with: uri)
    try embedded.write(to: file, atomically: true, encoding: .utf8)
}
