$( function ( mw ) {
	var api = new mw.Api(),
		windowManager = new OO.ui.WindowManager();

	// GoogleLogin javascript module
	mw.GoogleLogin = {
		/**
		 * Creates a new ProcessDialog as a Google Plus Info dialog with some information about
		 * the user connected with the provided Google Plus ID. The data will be loaded from Api
		 * and if there is an error, an error dialog will be shown. The user has always a button
		 * to open the Google Plus profile directly on Google Plus.
		 *
		 * @param {Integer} plusid The Google Plus ID to load the data from
		 */
		showInfoDialog: function ( plusid ) {
			// Create an information overlay to show basics about a google plus id
			function InfoDialog( config ) {
				InfoDialog.super.call( this, config );
			}
			// inherit ProcessDialog instead of MessageDialog or similar to use the top buttons
			OO.inheritClass( InfoDialog, OO.ui.ProcessDialog );

			InfoDialog.static.size = 'medium';
			InfoDialog.static.title = mw.msg( 'googlelogin-manage-plusinfo-title' );
			// one button to close the dialog and one to open a new tab
			// to redirect the user to the official and full google plus profile
			InfoDialog.static.actions = [
				{ action: 'close', label: 'Close', flags: 'safe' },
				{
					action: 'openplus',
					label: mw.msg( 'googlelogin-manage-openpluslink' ),
					flags: [ 'primary', 'safe' ],
					href: 'http://plus.google.com/' + plusid,
					target: '_blank'
				}
			];

			InfoDialog.prototype.getBodyHeight = function () {
				return 400;
			};

			// Make an api request to get the data and show the dialog.
			InfoDialog.prototype.initialize = function () {
				var self = this;

				InfoDialog.super.prototype.initialize.apply( this, arguments );
				this.content = new OO.ui.PanelLayout( {
					$: this.$,
					padded: true,
					scrollable: true
				} );
				// show, that we're loading the data
				this.pushPending();
				api.get( {
					action: 'googleplusprofileinfo',
					googleid: plusid
				} ).done( function ( data ) {
					if ( data && data.googleplusprofileinfo && data.googleplusprofileinfo.result ) {
						$.each( data.googleplusprofileinfo.result, function ( index, value ) {
							var element;

							if ( index !== 'profileimage' ) {
								// add a new information fieldset
								element = new OO.ui.FieldsetLayout( {
									$: self.$,
									label: index
								} );
								// determine if it is a boolean value and use yes/no
								if ( value === true || value === false ) {
									value = (
										value === true ?
										mw.msg( 'googlelogin-manage-yes' ) :
										mw.msg( 'googlelogin-manage-no' )
									);
								}
								element.addItems( [
									new OO.ui.LabelWidget( {
										$: self.$, label: value
									} )
								] );
								self.content.$element.append( element.$element );
							} else {
								// add the profile image if there exists one
								element = new OO.ui.LabelWidget( {
									$: self.$,
									label: $( '<img src="' + value + '">'  ),
									classes: [ 'googlelogin-profilepicture' ]
								} );
								self.content.$element.prepend( element.$element );
							}
						} );
					} else {
						var fieldset;
						fieldset = new OO.ui.FieldsetLayout( {
							$: self.$, label: 'Data could not be loaded', icon: 'alert'
						} );
						fieldset.addItems( [
							new OO.ui.LabelWidget( {
								$: self.$, label: 'Sorry, but there was an error.'
							} )
						] );
						self.content.$element.append( fieldset.$element );
					}
					self.$body.append( self.content.$element );

					// finished loading, hide loading
					self.popPending();
				} ).fail( function ( code, error ) {
					var msg;

					if ( error.info ) {
						msg = error.info;
					} else {
						msg = mw.msg( 'googlelogin-manage-unknownerror' );
					}
					self.close();
					var messageDialog = new OO.ui.MessageDialog();

					// Add the dialog to the window manager.
					windowManager.addWindows( [ messageDialog ] );

					// Configure the message dialog when it is opened with the window manager's openWindow() method.
					windowManager.openWindow( messageDialog, {
						title: mw.msg( 'googlelogin-manage-errorloading' ),
						message: msg,
						actions: [
							// the user should have the chance to easily open the google plus profile, here is the link
							{
								action: 'pluslink',
								label: mw.msg( 'googlelogin-manage-openpluslink' ),
								flags: 'primary',
								href: 'http://plus.google.com/' + plusid,
								target: '_blank'
							},
							// just close the dialog
							{ action: 'reject', label: mw.msg( 'googlelogin-manage-dismiss' ), flags: 'safe' }
						]
					});
				} );
			};

			InfoDialog.prototype.getActionProcess = function ( action ) {
				return InfoDialog.super.prototype.getActionProcess.call( this, action )
					.next(
						function () {
							if ( action === 'close' ) {
								this.close();
							}
							return InfoDialog.super.prototype.getActionProcess.call( this, action );
						},
						this
					);
			};

			// Create a new instance of InfoDialog
			var infoDialog = new InfoDialog();

			// and add it to the window manager
			windowManager.addWindows( [ infoDialog ] );
			windowManager.openWindow( infoDialog );
		}
	};

	// Append the window manager.
	$( 'body' ).append( windowManager.$element );

	$( '.googlelogin-googleid' ).toggleClass( 'hidden' ).on( 'click', function () {
		mw.GoogleLogin.showInfoDialog( $( this ).data( 'googleid' ) );
	} );
}( mediaWiki ) );