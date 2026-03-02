import Quartz
import Foundation

if CommandLine.arguments.count < 2 {
    print("Usage: extract_pdf.swift <pdf_path>")
    exit(1)
}
let path = CommandLine.arguments[1]
let url = URL(fileURLWithPath: path)
if let pdf = PDFDocument(url: url) {
    if let text = pdf.string {
        print(text)
    }
}
