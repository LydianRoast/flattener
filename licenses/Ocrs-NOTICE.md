# Ocrs contour postprocessing

The word-rectangle contour postprocessing in `src/ocr.rs` is adapted from Ocrs 0.13.1, `ocrs/src/detection.rs`, at upstream revision `e5f1c5205326804637368422fe420aa1b9769676`.

Source: https://github.com/robertknight/ocrs/blob/e5f1c5205326804637368422fe420aa1b9769676/ocrs/src/detection.rs

The adaptation uses the public probability-mask API and a 0.5 threshold appropriate to the bundled Hugging Face detector. It retains the upstream polygon simplification, rectangle expansion, and minimum-area settings. The threshold requirement is documented by the maintainer at https://github.com/robertknight/ocrs/issues/254#issuecomment-4620351611.

This source adaptation is used under the upstream MIT license, reproduced below. The separately bundled model weights remain subject to CC BY-SA 4.0.

Copyright (c) 2023 Ocrs project contributors.

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
