<?php

class UserMergeLogger implements IUserMergeLogger {

	/**
	 * Adds a merge log entry
	 *
	 * @todo Stop using this deprecated format
	 * @param User $performer
	 * @param User $oldUser
	 * @param User $newUser
	 */
	public function addMergeEntry( User $performer, User $oldUser, User $newUser ) {
		$log = new LogPage( 'usermerge' );
		$log->addEntry(
			'mergeuser',
			$performer->getUserPage(),
			'',
			[
				$oldUser->getName(), $oldUser->getId(),
				$newUser->getName(), $newUser->getId()
			],
			$performer
		);
	}

	/**
	 * Adds a user deletion log entry
	 *
	 * @todo Stop using this deprecated format
	 * @param User $perfomer
	 * @param User $oldUser
	 */
	public function addDeleteEntry( User $perfomer, User $oldUser ) {
		$log = new LogPage( 'usermerge' );
		$log->addEntry(
			'deleteuser',
			$perfomer->getUserPage(),
			'',
			[ $oldUser->getName(), $oldUser->getId() ],
			$perfomer
		);
	}
}
