import { describe, expect, it } from "vitest";
import { serviceBadge } from "./serviceBadge";

describe("serviceBadge", () => {
  const ids = ["caddy", "mariadb@main", "php-fpm@8.4", "php-fpm@7.4", "mysql@5.7"];

  it("draws every instance of a package from the package's name", () => {
    expect(serviceBadge("php-fpm@8.4", ids).name).toBe("php-fpm");
    expect(serviceBadge("php-fpm@7.4", ids).name).toBe("php-fpm");
  });

  it("tags the instance only when another service shares the package", () => {
    expect(serviceBadge("php-fpm@8.4", ids).tag).toBe("8.4");
    expect(serviceBadge("mysql@5.7", ids).tag).toBeUndefined();
    expect(serviceBadge("mariadb@main", ids).tag).toBeUndefined();
  });

  it("cuts a version instance to major.minor", () => {
    const versions = ["php-fpm@8.4.26", "php-fpm@7.4.33", "php-fpm@8", "redis@main"];
    expect(serviceBadge("php-fpm@8.4.26", versions).tag).toBe("8.4");
    expect(serviceBadge("php-fpm@8", versions).tag).toBe("8");
  });

  it("tags only a version, never an instance named for something else", () => {
    const shared = ["php-fpm@8.5.11", "php-fpm@phpmyadmin", "mariadb@main", "mariadb@reset"];
    expect(serviceBadge("php-fpm@8.5.11", shared).tag).toBe("8.5");
    expect(serviceBadge("php-fpm@phpmyadmin", shared).tag).toBeUndefined();
    expect(serviceBadge("mariadb@main", shared).tag).toBeUndefined();
  });

  it("takes an id without an instance as it is", () => {
    expect(serviceBadge("caddy", ids)).toEqual({ name: "caddy", tag: undefined });
    expect(serviceBadge("@odd", ids)).toEqual({ name: "@odd", tag: undefined });
  });
});
