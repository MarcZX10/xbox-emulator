import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import strip_comments as sc


def strip(s):
    return sc.strip_text(s)


class StripTests(unittest.TestCase):
    def test_line_doc_inner_comments(self):
        src = "//! crate doc\n/// item doc\n// plain\nfn a() {}\n    // indented\nlet x = 1; // trailing kept\n"
        out, n = strip(src)
        self.assertEqual(n, 4)
        self.assertEqual(out, "fn a() {}\nlet x = 1; // trailing kept\n")

    def test_url_in_string(self):
        src = 'let u = "https://learn.microsoft.com/x";\nlet v = "a\n// not a comment\nb";\n'
        out, n = strip(src)
        self.assertEqual(n, 0)
        self.assertEqual(out, src)

    def test_escaped_quote_in_string(self):
        src = 'let s = "he said \\"// hi\\"";\n// gone\nlet t = 2;\n'
        out, n = strip(src)
        self.assertEqual(n, 1)
        self.assertEqual(out, 'let s = "he said \\"// hi\\"";\nlet t = 2;\n')

    def test_raw_strings(self):
        src = 'let r = r#"\n// inside raw "quote"\n/* also */\n"#;\nlet q = r"//x";\nlet b = br##"\n// y "# still\n"##;\n// gone\n'
        out, n = strip(src)
        self.assertEqual(n, 1)
        self.assertEqual(out, src[: src.rindex("// gone")])

    def test_raw_identifier_not_string(self):
        src = 'let r#type = 1;\n// gone\nlet bring = "x";\n'
        out, n = strip(src)
        self.assertEqual(n, 1)
        self.assertEqual(out, 'let r#type = 1;\nlet bring = "x";\n')

    def test_nested_block_comments(self):
        src = "/* outer\n  /* inner */\n  still outer\n*/\nfn f() {}\nlet a = /* x */ 1;\n/* one */ /* two */\n/** doc block */\n"
        out, n = strip(src)
        self.assertEqual(n, 6)
        self.assertEqual(out, "fn f() {}\nlet a = /* x */ 1;\n")

    def test_block_comment_with_code_edges_kept(self):
        src = "let a = 1; /* start\n middle\n end */ let b = 2;\n"
        out, n = strip(src)
        self.assertEqual(n, 1)
        self.assertEqual(out, "let a = 1; /* start\n end */ let b = 2;\n")

    def test_doc_attributes_untouched(self):
        src = '#![doc = "// not a comment"]\n#[doc = "/* nor this */"]\nfn g() {}\n'
        out, n = strip(src)
        self.assertEqual(n, 0)
        self.assertEqual(out, src)

    def test_char_literals_and_lifetimes(self):
        src = "let q = '\"';\nlet e = '\\'';\nfn h<'a>(x: &'a str) -> char { '/' }\n// gone\nlet s = \"//\";\nlet u = '\\u{2F}';\n"
        out, n = strip(src)
        self.assertEqual(n, 1)
        self.assertEqual(out, src.replace("// gone\n", ""))

    def test_byte_and_c_strings(self):
        src = 'let b = b"// x";\nlet c = c"/* y */";\nlet d = b\'/\';\n// gone\n'
        out, n = strip(src)
        self.assertEqual(n, 1)
        self.assertEqual(out, src.replace("// gone\n", ""))

    def test_crlf_and_bom_preserved(self):
        src = "fn a() {}\r\n// gone\r\n    /// doc\r\nfn b() {}\r\n// last"
        data = sc.BOM + src.encode("utf-8")
        new, n = sc.strip_bytes(data)
        self.assertEqual(n, 3)
        self.assertEqual(new, sc.BOM + b"fn a() {}\r\nfn b() {}\r\n")
        plain, n2 = sc.strip_bytes(src.encode("utf-8"))
        self.assertFalse(plain.startswith(sc.BOM))
        self.assertEqual(n2, 3)

    def test_unicode_preserved(self):
        src = "let s = \"日本 // 語\";\n// ünïcödé\nlet c = '中';\n"
        out, n = strip(src)
        self.assertEqual(n, 1)
        self.assertEqual(out, "let s = \"日本 // 語\";\nlet c = '中';\n")

    def test_unterminated_is_error(self):
        with self.assertRaises(ValueError):
            sc.comment_mask("/* open\nfn a() {}\n")
        with self.assertRaises(ValueError):
            sc.comment_mask('let s = r#"never closed\n')

    def test_cli_dry_run_does_not_write(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "x.rs")
            with open(p, "wb") as h:
                h.write(b"// c\nfn a() {}\n")
            self.assertEqual(sc.main([d]), 0)
            with open(p, "rb") as h:
                self.assertEqual(h.read(), b"// c\nfn a() {}\n")
            self.assertEqual(sc.main([d, "--apply"]), 0)
            with open(p, "rb") as h:
                self.assertEqual(h.read(), b"fn a() {}\n")


if __name__ == "__main__":
    unittest.main()
