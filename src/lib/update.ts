// 开发测试阶段源码托管在 Gitee；远程更新与发行以 GitHub 仓库为准。
export const GITHUB_OWNER = "creatcode";
export const GITHUB_REPO = "AiAccountCenter";
export const GITHUB_REPOSITORY_URL = GITHUB_OWNER && GITHUB_REPO
  ? `https://github.com/${GITHUB_OWNER}/${GITHUB_REPO}`
  : "";
export const GITHUB_RELEASE_URL = GITHUB_REPOSITORY_URL
  ? `${GITHUB_REPOSITORY_URL}/releases/latest`
  : "";

/** 在桌面端通过 Tauri opener 打开，在 webui 端打开新标签页。 */
export async function openReleaseUrl(url = GITHUB_RELEASE_URL): Promise<void> {
  if (!url) return; // 更新源未配置：不提供任何下载入口
  const isWebui = typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);
  if (isWebui) {
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(url);
}
