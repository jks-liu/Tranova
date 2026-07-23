import { describe, expect, it } from "vitest";
import { nowIso, readJsonFile } from "./utils";

describe("JSON imports", () => {
  it("parses an exported JSON file", async () => {
    const file = new File([JSON.stringify([{ id: "one" }])], "export.json", { type: "application/json" });
    await expect(readJsonFile(file)).resolves.toEqual([{ id: "one" }]);
  });

  it("creates an ISO timestamp for saved records", () => {
    expect(new Date(nowIso()).toString()).not.toBe("Invalid Date");
  });
});
