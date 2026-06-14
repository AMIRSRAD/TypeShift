import { describe, expect, it } from "vitest";

function uniquePaths(paths: string[]) {
  return Array.from(new Set(paths.filter(Boolean)));
}

describe("queue helpers", () => {
  it("deduplicates paths without preserving empty values", () => {
    expect(uniquePaths(["a.heic", "", "a.heic", "b.heif"])).toEqual(["a.heic", "b.heif"]);
  });
});
