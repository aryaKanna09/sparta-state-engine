"""pptxgenjs writes a second <a:pPr> (with buNone) mid-paragraph when a bulleted item has several runs.
Keep only the first <a:pPr> of every paragraph so bullets survive in Google Slides / PowerPoint."""
import re, sys, zipfile, shutil
src = sys.argv[1]; tmp = src + ".tmp"
pp = re.compile(r"<a:pPr\b[^>]*?(?:/>|>.*?</a:pPr>)", re.S)
fixed = 0
with zipfile.ZipFile(src) as zin, zipfile.ZipFile(tmp, "w", zipfile.ZIP_DEFLATED) as zout:
    for it in zin.infolist():
        data = zin.read(it.filename)
        if re.match(r"ppt/slides/slide\d+\.xml$", it.filename):
            x = data.decode()
            def para(m):
                global fixed
                p = m.group(0); seen = [False]
                def one(mm):
                    global fixed
                    if seen[0]: fixed += 1; return ""
                    seen[0] = True; return mm.group(0)
                return pp.sub(one, p)
            x = re.sub(r"<a:p>.*?</a:p>", para, x, flags=re.S)
            data = x.encode()
        zout.writestr(it, data)
shutil.move(tmp, src); print("removed stray pPr:", fixed)
