/*!
 * VisualEditor DataModel MWReferencesListNode class.
 *
 * @copyright 2011-2017 Cite VisualEditor Team and others; see AUTHORS.txt
 * @license The MIT License (MIT); see LICENSE.txt
 */

/**
 * DataModel MediaWiki references list node.
 *
 * @class
 * @extends ve.dm.BranchNode
 * @mixins ve.dm.FocusableNode
 *
 * @constructor
 * @param {Object} [element] Reference to element in linear model
 * @param {ve.dm.Node[]} [children]
 */
ve.dm.MWReferencesListNode = function VeDmMWReferencesListNode() {
	// Parent constructor
	ve.dm.MWReferencesListNode.super.apply( this, arguments );

	// Mixin constructors
	ve.dm.FocusableNode.call( this );
};

/* Inheritance */

OO.inheritClass( ve.dm.MWReferencesListNode, ve.dm.BranchNode );

OO.mixinClass( ve.dm.MWReferencesListNode, ve.dm.FocusableNode );

/* Static members */

ve.dm.MWReferencesListNode.static.name = 'mwReferencesList';

ve.dm.MWReferencesListNode.static.handlesOwnChildren = true;

ve.dm.MWReferencesListNode.static.ignoreChildren = true;

ve.dm.MWReferencesListNode.static.matchTagNames = null;

ve.dm.MWReferencesListNode.static.matchRdfaTypes = [ 'mw:Extension/references' ];

ve.dm.MWReferencesListNode.static.preserveHtmlAttributes = false;

ve.dm.MWReferencesListNode.static.toDataElement = function ( domElements, converter ) {
	var referencesListData, contentsDiv, contentsData,
		isResponsiveDefault = mw.config.get( 'wgCiteResponsiveReferences' ),
		mwDataJSON = domElements[ 0 ].getAttribute( 'data-mw' ),
		mwData = mwDataJSON ? JSON.parse( mwDataJSON ) : {},
		refGroup = ve.getProp( mwData, 'attrs', 'group' ) || '',
		responsiveAttr = ve.getProp( mwData, 'attrs', 'responsive' ),
		listGroup = 'mwReference/' + refGroup;

	referencesListData = {
		type: this.name,
		attributes: {
			mw: mwData,
			originalMw: mwDataJSON,
			refGroup: refGroup,
			listGroup: listGroup,
			isResponsive: responsiveAttr !== undefined ? responsiveAttr !== '0' : isResponsiveDefault
		}
	};
	if ( mwData.body && mwData.body.html ) {
		// Process the nodes in .body.html as if they were this node's children
		contentsDiv = domElements[ 0 ].ownerDocument.createElement( 'div' );
		contentsDiv.innerHTML = mwData.body.html;
		contentsData = converter.getDataFromDomClean( contentsDiv );
		referencesListData = [ referencesListData ]
			.concat( contentsData )
			.concat( [ { type: '/' + this.name } ] );
	}
	return referencesListData;
};

ve.dm.MWReferencesListNode.static.toDomElements = function ( data, doc, converter ) {
	var el, els, mwData, originalMw, contentsHtml, originalHtml,
		isResponsiveDefault = mw.config.get( 'wgCiteResponsiveReferences' ),
		wrapper = doc.createElement( 'div' ),
		originalHtmlWrapper = doc.createElement( 'div' ),
		dataElement = data[ 0 ],
		attrs = dataElement.attributes,
		contentsData = data.slice( 1, -1 );

	if ( dataElement.originalDomElementsIndex !== undefined ) {
		// If there's more than 1 element, preserve entire array, not just first element
		els = ve.copyDomElements( converter.getStore().value( dataElement.originalDomElementsIndex ), doc );
	} else {
		els = [ doc.createElement( 'div' ) ];
	}

	mwData = attrs.mw ? ve.copy( attrs.mw ) : {};

	mwData.name = 'references';

	if ( attrs.refGroup ) {
		ve.setProp( mwData, 'attrs', 'group', attrs.refGroup );
	} else if ( mwData.attrs ) {
		delete mwData.attrs.refGroup;
	}

	if ( attrs.isResponsive !== isResponsiveDefault ) {
		ve.setProp( mwData, 'attrs', 'responsive', attrs.isResponsive ? '' : '0' );
	} else if ( mwData.attrs ) {
		delete mwData.attrs.responsive;
	}

	el = els[ 0 ];
	el.setAttribute( 'typeof', 'mw:Extension/references' );

	if ( contentsData.length > 2 ) {
		converter.getDomSubtreeFromData( data.slice( 1, -1 ), wrapper );
		contentsHtml = wrapper.innerHTML; // Returns '' if wrapper is empty
		originalHtml = ve.getProp( mwData, 'body', 'html' ) || '';
		originalHtmlWrapper.innerHTML = originalHtml;
		// Only set body.html if contentsHtml and originalHtml are actually different
		if ( !originalHtmlWrapper.isEqualNode( wrapper ) ) {
			ve.setProp( mwData, 'body', 'html', contentsHtml );
		}
	}

	// If mwData and originalMw are the same, use originalMw to prevent reserialization.
	// Reserialization has the potential to reorder keys and so change the DOM unnecessarily
	originalMw = attrs.originalMw;
	if ( originalMw && ve.compare( mwData, JSON.parse( originalMw ) ) ) {
		el.setAttribute( 'data-mw', originalMw );
	} else {
		el.setAttribute( 'data-mw', JSON.stringify( mwData ) );
	}

	return els;
};

ve.dm.MWReferencesListNode.static.describeChange = function ( key, change ) {
	if ( key === 'refGroup' ) {
		if ( change.from ) {
			if ( change.to ) {
				return ve.msg( 'cite-ve-changedesc-ref-group-both', change.from, change.to );
			} else {
				return ve.msg( 'cite-ve-changedesc-ref-group-from', change.from );
			}
		}
		return ve.msg( 'cite-ve-changedesc-ref-group-to', change.to );
	}
	if ( key === 'originalMw' ) {
		return null;
	}

	return null;
};

/* Registration */

ve.dm.modelRegistry.register( ve.dm.MWReferencesListNode );
