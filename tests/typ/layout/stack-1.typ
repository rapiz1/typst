// Test stack layouts.

---
// Test stacks with different directions.
#let widths = (
  30pt, 20pt, 40pt, 15pt,
  30pt, 50%, 20pt, 100%,
)

#let shaded = {
  let v = 0
  let next() = { v += 0.1; rgb(v, v, v) }
  w => rect(width: w, height: 10pt, fill: next())
}

#let items = for w in widths { (align(right, shaded(w)),) }

#page(width: 50pt, margins: 0pt)
#stack(dir: btt, ..items)

---
// Test RTL alignment.
#page(width: 50pt, margins: 5pt)
#font(8pt)
#stack(dir: rtl,
  align(center, [A]),
  align(left, [B]),
  [C],
)

---
// Test spacing.
#page(width: 50pt, margins: 0pt)
#par(spacing: 5pt)

#let x = square(size: 10pt, fill: eastern)
#stack(dir: rtl, spacing: 5pt, x, x, x)
#stack(dir: ltr, x, 20%, x, 20%, x)
#stack(dir: ltr, spacing: 5pt, x, x, 7pt, 3pt, x)

---
// Test overflow.
#page(width: 50pt, height: 30pt, margins: 0pt)
#box(stack(
  rect(width: 40pt, height: 20pt, fill: conifer),
  rect(width: 30pt, height: 13pt, fill: forest),
))