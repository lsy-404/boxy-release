const requiredAssets = ["boxy-windows-x64.exe", "boxy-macos-universal.zip"];

function releaseTime(release) {
  const value = release.published_at ?? release.created_at;
  const time = Date.parse(value);
  if (!Number.isFinite(time)) {
    throw new Error(`Release ${release.tag_name} has no valid publication time.`);
  }
  return time;
}

export async function retainLatestRelease({ tag, api }) {
  const current = await api.getReleaseByTag(tag);
  if (current.tag_name !== tag || current.draft || !current.published_at) {
    throw new Error(`Release ${tag} is missing, unpublished, or does not match the requested tag.`);
  }

  const assets = new Map((current.assets ?? []).map((asset) => [asset.name, asset]));
  for (const name of requiredAssets) {
    const asset = assets.get(name);
    if (!asset || asset.state !== "uploaded" || asset.size <= 0) {
      throw new Error(`Release ${tag} is missing a complete ${name} asset.`);
    }
  }

  const releases = await api.listReleases();
  const currentTime = releaseTime(current);
  const olderReleases = releases.filter((release) =>
    release.tag_name !== tag && releaseTime(release) < currentTime,
  );
  for (const release of olderReleases) {
    await api.deleteRelease(release.id);
  }
  return { deleted: olderReleases.map(({ tag_name }) => tag_name) };
}
