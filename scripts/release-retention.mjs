function parseVersion(tag) {
  const match = /^v(\d+)\.(\d+)\.(\d+)(?:[.-]([A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*))?$/.exec(tag);
  if (!match) return null;
  return { numbers: match.slice(1, 4).map(BigInt), prerelease: match[4]?.split(".") ?? [] };
}

function compareIdentifier(left, right) {
  if (/^\d+$/.test(left) && /^\d+$/.test(right)) {
    return BigInt(left) < BigInt(right) ? -1 : BigInt(left) > BigInt(right) ? 1 : 0;
  }
  if (/^\d+$/.test(left) !== /^\d+$/.test(right)) return /^\d+$/.test(left) ? -1 : 1;
  return left < right ? -1 : left > right ? 1 : 0;
}

function isOlder(tag, current) {
  const version = parseVersion(tag);
  if (!version) return false;
  for (let index = 0; index < 3; index++) {
    if (version.numbers[index] !== current.numbers[index]) {
      return version.numbers[index] < current.numbers[index];
    }
  }
  if (!version.prerelease.length || !current.prerelease.length) {
    return version.prerelease.length > 0 && current.prerelease.length === 0;
  }
  for (let index = 0; index < Math.max(version.prerelease.length, current.prerelease.length); index++) {
    if (version.prerelease[index] === undefined) return true;
    if (current.prerelease[index] === undefined) return false;
    const order = compareIdentifier(version.prerelease[index], current.prerelease[index]);
    if (order !== 0) return order < 0;
  }
  return false;
}

export async function retainLatestRelease({ tag, api }) {
  const currentVersion = parseVersion(tag);
  if (!currentVersion) throw new Error(`Release ${tag} has an unsupported version tag.`);
  const current = await api.getReleaseByTag(tag);
  if (current.tag_name !== tag || current.draft || !current.published_at) {
    throw new Error(`Release ${tag} is missing, unpublished, or does not match the requested tag.`);
  }

  const releases = await api.listReleases();
  const tags = await api.listTags();
  const olderReleases = releases.filter((release) => isOlder(release.tag_name, currentVersion));
  const olderTags = new Set([
    ...olderReleases.map((release) => release.tag_name),
    ...tags.filter(({ name }) => isOlder(name, currentVersion)).map(({ name }) => name),
  ]);
  for (const release of olderReleases) {
    await api.deleteRelease(release.id);
  }
  const deletedTags = [];
  for (const oldTag of olderTags) {
    try {
      await api.deleteTag(oldTag);
      deletedTags.push(oldTag);
    } catch (error) {
      if (error.status !== 404) throw error;
    }
  }
  return { deleted: olderReleases.map(({ tag_name }) => tag_name), deletedTags };
}
