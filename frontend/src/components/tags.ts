// 로스터·워커 섹션이 공유하는 태그 표시 헬퍼(컴포넌트 파일에서 분리 = fast refresh 호환).

// 태그 표시 순서. 없는 키는 뒤에 알파벳순.
export const TAG_ORDER = ['machine', 'runner', 'role', 'project']

export function orderedTags(tags: Record<string, string>): Array<[string, string]> {
  const known = TAG_ORDER.filter((k) => k in tags).map((k) => [k, tags[k]] as [string, string])
  const rest = Object.keys(tags)
    .filter((k) => !TAG_ORDER.includes(k))
    .sort()
    .map((k) => [k, tags[k]] as [string, string])
  return [...known, ...rest]
}
