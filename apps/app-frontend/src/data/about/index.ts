import contributorsData from './contributors.json'
import teamData from './team.json'

export interface TeamMember {
	name: string
	avatar: string
	avatarRemote?: string
	url?: string
	experience?: string
}

export interface Contributor {
	name: string
	avatarUrl: string
	url: string
	contributions: number
}

const teamAvatarModules = import.meta.glob('./avatars/*', {
	eager: true,
	import: 'default',
	query: '?url',
}) as Record<string, string>

export const teamMembers: (TeamMember & { avatarUrl: string })[] = teamData.map((member) => ({
	...member,
	avatarUrl: teamAvatarModules[`./avatars/${member.avatar}`],
}))

export const contributors = contributorsData as Contributor[]

export const ysumclTeamMembers: (TeamMember & { avatarUrl: string })[] = [
    {
        name: 'GoodNormal',
        avatar: 'goodnormal.png',
        avatarUrl: teamAvatarModules['./avatars/goodnormal.png'],
        url: 'https://github.com/GoodNormal',
    },
    {
        name: 'wotsginger',
        avatar: 'wotsginger.jpg',
        avatarUrl: teamAvatarModules['./avatars/wotsginger.jpg'],
        url: 'https://github.com/wotsginger',
    },
]
