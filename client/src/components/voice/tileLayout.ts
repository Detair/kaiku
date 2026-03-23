/** Result of the square-fit grid calculation. */
export interface GridLayout {
  cols: number;
  rows: number;
  tileWidth: number;
  tileHeight: number;
}

const ASPECT_RATIO = 4 / 3;
const MIN_TILE_WIDTH = 120;
const MAX_COLS = 5;

/**
 * Calculate optimal grid dimensions for n tiles in a container of W x H.
 * Tries each column count and picks the one that maximizes coverage
 * (least wasted space) with a uniform 4:3 aspect ratio.
 */
export function calculateGrid(n: number, containerWidth: number, containerHeight: number): GridLayout {
  if (n === 0) return { cols: 0, rows: 0, tileWidth: 0, tileHeight: 0 };

  let bestCols = 1;
  let bestCoverage = 0;

  for (let cols = 1; cols <= Math.min(n, MAX_COLS); cols++) {
    const rows = Math.ceil(n / cols);
    const tileW = containerWidth / cols;
    const tileH = containerHeight / rows;
    const actualW = Math.min(tileW, tileH * ASPECT_RATIO);
    const actualH = actualW / ASPECT_RATIO;

    if (cols > 1 && actualW < MIN_TILE_WIDTH) continue;

    const coverage = (actualW * actualH * n) / (containerWidth * containerHeight);
    if (coverage > bestCoverage) {
      bestCoverage = coverage;
      bestCols = cols;
    }
  }

  const rows = Math.ceil(n / bestCols);
  const tileW = containerWidth / bestCols;
  const tileH = containerHeight / rows;
  const tileWidth = Math.min(tileW, tileH * ASPECT_RATIO);
  const tileHeight = tileWidth / ASPECT_RATIO;

  return { cols: bestCols, rows, tileWidth, tileHeight };
}
