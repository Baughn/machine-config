/**
 * JavaScript for the CategoryTree extension.
 *
 * @file
 * @ingroup Extensions
 * @author Daniel Kinzler, brightbyte.de
 * @copyright © 2006 Daniel Kinzler
 * @licence GNU General Public Licence 2.0 or later
 */

( function ( $, mw ) {

	mw.hook( 'wikipage.content' ).add( function ( $content ) {

		/**
		 * Sets display inline to tree toggle
		 */
		function showToggles() {
			$content.find( 'span.CategoryTreeToggle' ).css( 'display', 'inline' );
		}

		/**
		 * Handles clicks on the expand buttons, and calls the appropriate function
		 *
		 * @context {Element} CategoryTreeToggle
		 * @param e {jQuery.Event}
		 */
		function handleNode( /*e*/ ) {
			var $link = $( this );
			if ( $link.data( 'ct-state' ) === 'collapsed' ) {
				expandNode( $link );
			} else {
				collapseNode( $link );
			}
		}

		/**
		 * Expands a given node (loading it's children if not loaded)
		 *
		 * @param {jQuery} $link
		 */
		function expandNode( $link ) {
			// Show the children node
			var $children = $link.parents( '.CategoryTreeItem' )
					.siblings( '.CategoryTreeChildren' );
			$children.show();

			$link
				.text( mw.msg( 'categorytree-collapse-bullet' ) )
				.attr( 'title', mw.msg( 'categorytree-collapse' ) )
				.data( 'ct-state', 'expanded' );

			if ( !$link.data( 'ct-loaded' ) ) {
				loadChildren( $link, $children );
			}
		}

		/**
		 * Collapses a node
		 *
		 * @param {jQuery} $link
		 */
		function collapseNode( $link ) {
			// Hide the children node
			$link.parents( '.CategoryTreeItem' )
				.siblings( '.CategoryTreeChildren' ).hide();

			$link
				.text( mw.msg( 'categorytree-expand-bullet' ) )
				.attr( 'title', mw.msg( 'categorytree-expand' ) )
				.data( 'ct-state', 'collapsed' );
		}

		/**
		 * Loads children for a node via an HTTP call
		 *
		 * @param {jQuery} $link
		 * @param {jQuery} $children
		 */
		function loadChildren( $link, $children ) {
			var $linkParentCTTag, ctTitle, ctMode, ctOptions;

			/**
			 * Error callback
			 */
			function error() {
				var $retryLink;

				$retryLink = $( '<a>' )
					.text( mw.msg( 'categorytree-retry' ) )
					.attr( 'href', '#' )
					.click( function ( e ) {
						e.preventDefault();
						loadChildren( $link, $children );
					} );

				$children
					.text( mw.msg( 'categorytree-error' ) + ' ' )
					.append( $retryLink );
			}

			$link.data( 'ct-loaded', true );

			$children.append(
				$( '<i class="CategoryTreeNotice"></i>' )
					.text( mw.msg( 'categorytree-loading' ) )
			);

			$linkParentCTTag = $link.parents( '.CategoryTreeTag' );

			// Element may not have a .CategoryTreeTag parent, fallback to defauls
			// Probably a CategoryPage (@todo: based on what?)
			ctTitle = $link.data( 'ct-title' );
			ctMode = $linkParentCTTag.data( 'ct-mode' );
			ctMode = typeof ctMode === 'number' ? ctMode : undefined;
			ctOptions = JSON.stringify( $linkParentCTTag.data( 'ct-options' ) );
			if ( !ctOptions ) {
				ctOptions = mw.config.get( 'wgCategoryTreePageCategoryOptions' );
			}

			// Mode and options have defaults or fallbacks, title does not.
			// Don't make a request if there is no title.
			if ( typeof ctTitle !== 'string' ) {
				error();
				return;
			}

			new mw.Api().get( {
				action: 'categorytree',
				category: ctTitle,
				options: ctOptions,
				uselang: mw.config.get( 'wgUserLanguage' ),
				formatversion: 2
			} ).done( function ( data ) {
				data = data.categorytree.html;

				if ( data === '' ) {
					switch ( ctMode ) {
						// CategoryTreeMode::CATEGORIES = 0
						case 0:
							data = mw.msg( 'categorytree-no-subcategories' );
							break;
						// CategoryTreeMode::PAGES = 10
						case 10:
							data = mw.msg( 'categorytree-no-pages' );
							break;
						// CategoryTreeMode::PARENTS = 100
						case 100:
							data = mw.msg( 'categorytree-no-parent-categories' );
							break;
						// CategoryTreeMode::ALL = 20
						default:
							data = mw.msg( 'categorytree-nothing-found' );
					}

					data = $( '<i class="CategoryTreeNotice"></i>' ).text( data );
				}

				$children
					.html( data )
					.find( '.CategoryTreeToggle' )
						.click( handleNode );

				showToggles();
			} )
			.fail( error );
		}

		// Register click events and show toggle buttons
		$content.find( '.CategoryTreeToggle' ).click( handleNode );
		showToggles();
	} );

}( jQuery, mediaWiki ) );
